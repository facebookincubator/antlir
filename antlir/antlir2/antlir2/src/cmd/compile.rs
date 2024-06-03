/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::path::PathBuf;

use antlir2_btrfs::Subvolume;
use antlir2_compile::Arch;
use antlir2_compile::CompileFeature;
use antlir2_compile::CompilerContext;
use antlir2_depgraph::Graph;
use antlir2_rootless::Rootless;
use antlir2_working_volume::WorkingVolume;
use anyhow::Context;
use buck_label::Label;
use clap::Parser;
use json_arg::JsonFile;
use nix::mount::mount;
use nix::mount::MsFlags;
use nix::sched::unshare;
use nix::sched::CloneFlags;
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

    #[clap(long)]
    /// Architecture of the image being built
    target_arch: Arch,

    #[clap(long)]
    /// Path to input depgraph with features to include in this image
    depgraph: PathBuf,

    #[clap(long)]
    /// Pre-computed plans for this compilation phase
    plans: JsonFile<HashMap<String, PathBuf>>,
}

impl Compile {
    #[tracing::instrument(name = "compile", skip(self, rootless), ret, err)]
    pub(crate) fn run(self, rootless: Rootless) -> Result<()> {
        let rootless = match self.rootless {
            true => None,
            false => Some(rootless),
        };

        if self.rootless {
            antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
        }

        let working_volume = WorkingVolume::ensure(self.working_dir.clone())
            .context("while setting up WorkingVolume")?;

        let mut subvol = self.create_new_subvol(&working_volume, &rootless)?;

        let root_guard = rootless.map(|r| r.escalate()).transpose()?;

        // Be careful to isolate this process from the host mount namespace in
        // case anything weird is going on
        unshare(CloneFlags::CLONE_NEWNS).context("while unsharing mount")?;

        // Remount / as private so that we don't let any changes escape back
        // to the parent mount namespace (basically equivalent to `mount
        // --make-rprivate /`)
        mount(
            None::<&str>,
            "/",
            None::<&str>,
            MsFlags::MS_REC | MsFlags::MS_PRIVATE,
            None::<&str>,
        )
        .context("while making mount ns private")?;

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
        let ctx = self.compiler_context(subvol.path().to_owned(), plans)?;

        let depgraph = Graph::open(self.depgraph).context("while opening depgraph")?;

        let root_guard = rootless.map(|r| r.escalate()).transpose()?;
        for feature in depgraph
            .pending_features()
            .context("while fetching pending features")?
        {
            feature.compile(&ctx)?;
        }
        drop(root_guard);

        debug!("compile finished, making subvol {subvol:?} readonly");

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
        std::os::unix::fs::symlink(subvol.path(), &self.output).context("while making symlink")?;

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
    fn create_new_subvol(
        &self,
        working_volume: &WorkingVolume,
        rootless: &Option<antlir2_rootless::Rootless>,
    ) -> Result<Subvolume> {
        let dst = working_volume
            .allocate_new_path()
            .context("while allocating new path for subvol")?;
        let _guard = rootless.map(|r| r.escalate()).transpose()?;
        let subvol = match &self.parent {
            Some(parent) => {
                trace!("snapshotting parent {parent:?}");
                let parent = Subvolume::open(parent)?;
                parent.snapshot(&dst, Default::default())?
            }
            None => Subvolume::create(&dst).context("while creating new subvol")?,
        };
        debug!("produced r/w subvol '{subvol:?}'");
        Ok(subvol)
    }
}
