/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::path::Path;
use std::path::PathBuf;

use antlir2_btrfs::Subvolume;
use antlir2_compile::Arch;
use antlir2_compile::CompileFeature;
use antlir2_compile::CompilerContext;
use antlir2_features::Feature;
use antlir2_overlayfs::OverlayFs;
use antlir2_rootless::Rootless;
use antlir2_working_volume::WorkingVolume;
use anyhow::anyhow;
use anyhow::Context;
use buck_label::Label;
use clap::Parser;
use clap::ValueEnum;
use fbinit::FacebookInit;
use json_arg::JsonFile;
use tracing::debug;
use tracing::trace;
use tracing::warn;

use crate::Error;
use crate::Result;

#[derive(Parser, Debug)]
/// Compile image features into a directory
pub(crate) struct Compile {
    #[clap(long)]
    /// Label of the image being built
    label: Label,
    #[clap(long)]
    /// Use an unprivileged usernamespace
    rootless: bool,

    #[clap(long)]
    /// Path to the working volume where images should be built
    working_dir: PathBuf,
    #[clap(long)]
    /// Path to a subvolume to use as the starting point
    parent: Option<PathBuf>,
    #[clap(long)]
    /// buck-out path to store the reference to this volume
    output: PathBuf,

    #[clap(value_enum, long, default_value_t=WorkingFormat::Btrfs)]
    /// On-disk format of the layer storage
    working_format: WorkingFormat,

    #[clap(long)]
    /// Architecture of the image being built
    target_arch: Arch,

    #[clap(long)]
    /// Path to features to build into this image
    features: JsonFile<Vec<Feature>>,

    #[clap(long)]
    /// Pre-computed plans for this compilation phase
    plans: JsonFile<HashMap<String, PathBuf>>,
}

#[derive(Debug, ValueEnum, Clone, Copy)]
enum WorkingFormat {
    Btrfs,
    Overlayfs,
}

#[derive(Debug)]
enum WorkingLayer {
    Btrfs(Subvolume),
    OverlayFs(OverlayFs),
}

impl WorkingLayer {
    fn path(&self) -> &Path {
        match self {
            WorkingLayer::Btrfs(subvol) => subvol.path(),
            WorkingLayer::OverlayFs(fs) => fs.mountpoint(),
        }
    }
}

impl Compile {
    #[tracing::instrument(name = "compile", skip_all, ret, err)]
    pub(crate) fn run(self, rootless: Rootless, fb: FacebookInit) -> Result<()> {
        // this must happen before unshare
        let working_volume = match self.working_format {
            WorkingFormat::Btrfs => Some(WorkingVolume::ensure(self.working_dir.clone())?),
            WorkingFormat::Overlayfs => None,
        };

        let rootless = match self.rootless {
            true => None,
            false => Some(rootless),
        };

        if self.rootless {
            antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
        }

        let root_guard = rootless.map(|r| r.escalate()).transpose()?;

        antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;

        let layer = self.create_new_layer(working_volume.as_ref(), &rootless)?;

        drop(root_guard);

        let plans = self
            .plans
            .as_inner()
            .iter()
            .map(|(id, path)| {
                let plan = std::fs::read_to_string(path)
                    .with_context(|| format!("while reading plan '{}'", path.display()))?;
                let plan: serde_json::Value = serde_json::from_str(&plan)
                    .with_context(|| format!("while parsing plan '{}'", path.display()))?;
                Result::Ok((id.to_owned(), plan))
            })
            .collect::<Result<_>>()?;
        let ctx = self.compiler_context(layer.path().to_owned(), plans)?;

        let root_guard = rootless.map(|r| r.escalate()).transpose()?;
        for feature in self.features.as_inner() {
            feature.compile(&ctx)?;
        }
        drop(root_guard);

        match layer {
            WorkingLayer::Btrfs(mut subvol) => {
                let root_guard = rootless.map(|r| r.escalate()).transpose()?;
                if self.output.exists() {
                    trace!("removing existing output {}", self.output.display());
                    // Don't fail if the old subvol couldn't be deleted, just print
                    // a warning. We really don't want to fail someone's build if
                    // the only thing that went wrong is not being able to delete
                    // the last version of it.
                    match Subvolume::open(&self.output) {
                        Ok(old_subvol) => {
                            if let Err(e) = old_subvol.delete() {
                                warn!(
                                    "couldn't delete old subvol '{}': {e:?}",
                                    self.output.display()
                                );
                            }
                        }
                        Err(e) => {
                            warn!(
                                "couldn't open old subvol '{}': {e:?}",
                                self.output.display()
                            );
                        }
                    }
                }

                debug!("compile finished, making subvol {subvol:?} readonly");

                subvol
                    .set_readonly(true)
                    .context("while making subvol r/o")?;

                debug!(
                    "linking {} -> {}",
                    self.output.display(),
                    subvol.path().display(),
                );
                drop(root_guard);

                let _ = std::fs::remove_file(&self.output);
                std::os::unix::fs::symlink(subvol.path(), &self.output)
                    .context("while making symlink")?;

                #[cfg(facebook)]
                working_volume
                    .as_ref()
                    .expect("WorkingVolume always exists for btrfs")
                    .log_to_scuba(fb);
            }
            WorkingLayer::OverlayFs(fs) => {
                drop(ctx);
                fs.finalize().context("while finalizing overlayfs")?;
            }
        }

        Ok(())
    }

    fn compiler_context(
        &self,
        root: PathBuf,
        plans: HashMap<String, serde_json::Value>,
    ) -> Result<CompilerContext> {
        CompilerContext::new(self.label.clone(), self.target_arch, root, plans)
            .map_err(Error::Compile)
    }

    /// Create a new mutable subvolume
    #[tracing::instrument(skip(self), ret, err)]
    fn create_new_layer(
        &self,
        working_volume: Option<&WorkingVolume>,
        rootless: &Option<antlir2_rootless::Rootless>,
    ) -> Result<WorkingLayer> {
        match self.working_format {
            WorkingFormat::Btrfs => {
                let dst = working_volume
                    .context("working_volume must have been created for btrfs")?
                    .allocate_new_path()
                    .context("while allocating new path for subvol")?;
                let _guard = rootless.map(|r| r.escalate()).transpose()?;
                let subvol = match &self.parent {
                    Some(parent) => {
                        trace!("snapshotting parent {parent:?}");
                        let parent = Subvolume::open(parent)?;
                        parent.snapshot(&dst, Default::default())?
                    }
                    None => Subvolume::create(&dst)?,
                };
                debug!("produced r/w subvol '{subvol:?}'");
                Ok(WorkingLayer::Btrfs(subvol))
            }
            WorkingFormat::Overlayfs => {
                if self.parent.is_some() {
                    return Err(anyhow!("overlayfs encodes parent in --output").into());
                }

                let model =
                    std::fs::read_to_string(&self.output).context("while reading model json")?;
                let model = serde_json::from_str(&model).context("while parsing model json")?;
                let opts = antlir2_overlayfs::Opts::builder().model(model).build();
                let fs = OverlayFs::mount(opts).context("while mounting overlayfs")?;
                Ok(WorkingLayer::OverlayFs(fs))
            }
        }
    }
}
