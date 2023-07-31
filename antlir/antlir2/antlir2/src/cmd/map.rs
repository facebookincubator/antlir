/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::isolate;
use antlir2_isolate::IsolationContext;
use antlir2_working_volume::WorkingVolume;
use anyhow::Context;
use btrfs::DeleteFlags;
use btrfs::SnapshotFlags;
use btrfs::Subvolume;
use buck_label::Label;
use clap::Parser;
use tracing::debug;

use super::compile::Compile;
use super::compile::CompileExternal;
use super::plan::Plan;
use super::plan::PlanExternal;
use super::Compileish;
use super::CompileishExternal;
use crate::Result;

#[derive(Parser, Debug)]
/// Map one image into another by running some 'antlir2' command in an isolated
/// environment.
pub(crate) struct Map {
    #[clap(long)]
    /// Label of the image being built
    label: Label,
    #[clap(flatten)]
    setup: SetupArgs,
    #[clap(long)]
    /// Path to mounted build appliance image
    build_appliance: PathBuf,
    /// Arguments to pass to the isolated instance of 'antlir2'
    #[clap(subcommand)]
    subcommand: Subcommand,
}

#[derive(Parser, Debug)]
struct SetupArgs {
    #[clap(long)]
    /// Path to the working volume where images should be built
    working_dir: PathBuf,
    #[clap(long)]
    /// Path to a subvolume to use as the starting point
    parent: Option<PathBuf>,
    /// Name for this mapping operation, applied to the internal subvolume
    /// created.
    /// Each [Label] can have many identifiers, but these must be unique within
    /// a single [Label].
    #[clap(long)]
    identifier: String,
    #[clap(long)]
    /// buck-out path to store the reference to this volume
    output: PathBuf,
    #[clap(flatten)]
    dnf: super::DnfCompileishArgs,
    #[cfg(facebook)]
    #[clap(flatten)]
    fbpkg: super::FbpkgCompileishArgs,
}

#[derive(Parser, Debug)]
enum Subcommand {
    Compile {
        #[clap(flatten)]
        compileish: CompileishExternal,
        #[clap(flatten)]
        external: CompileExternal,
    },
    Plan {
        #[clap(flatten)]
        compileish: CompileishExternal,
        #[clap(flatten)]
        external: PlanExternal,
    },
}

impl Subcommand {
    fn writable_outputs(&self) -> Result<BTreeSet<&Path>> {
        match self {
            Self::Plan {
                compileish: _,
                external,
            } => {
                std::fs::File::create(&external.plan).with_context(|| {
                    format!(
                        "while creating plan output file '{}'",
                        external.plan.display()
                    )
                })?;
                Ok(BTreeSet::from([external.plan.as_path()]))
            }
            _ => Ok(BTreeSet::new()),
        }
    }
}

impl Map {
    /// Create a new mutable subvolume based on the [SetupArgs].
    #[tracing::instrument(skip(self), ret, err)]
    fn create_new_subvol(&self, working_volume: &WorkingVolume) -> Result<Subvolume> {
        if self.setup.output.exists() {
            let subvol =
                Subvolume::get(&self.setup.output).context("while opening existing subvol")?;
            subvol
                .delete(DeleteFlags::RECURSIVE)
                .context("while deleting existing subvol")?;
            std::fs::remove_file(&self.setup.output).context("while deleting existing symlink")?;
        }
        let dst = working_volume.join(uuid::Uuid::new_v4().as_simple().to_string());
        let subvol = match &self.setup.parent {
            Some(parent) => {
                let parent = Subvolume::get(parent).context("while opening parent subvol")?;
                parent
                    .snapshot(&dst, SnapshotFlags::RECURSIVE)
                    .context("while snapshotting parent")?
            }
            None => Subvolume::create(&dst).context("while creating new subvol")?,
        };
        debug!("produced r/w subvol '{subvol:?}'");
        Ok(subvol)
    }

    #[tracing::instrument(name = "map", skip(self))]
    pub(crate) fn run(self, log_path: Option<&Path>) -> Result<()> {
        let working_volume = WorkingVolume::ensure(self.setup.working_dir.clone())
            .context("while setting up WorkingVolume")?;
        let mut subvol = self.create_new_subvol(&working_volume)?;

        let repo = find_root::find_repo_root(
            &absolute_path::AbsolutePathBuf::canonicalize(
                std::env::current_exe().context("while getting argv[0]")?,
            )
            .context("argv[0] not absolute")?,
        )
        .context("while looking for repo root")?;

        let mut writable_outputs = self
            .subcommand
            .writable_outputs()
            .context("while preparing writable outputs")?;
        writable_outputs.insert(working_volume.path());
        if let Some(path) = log_path {
            writable_outputs.insert(path);
        }

        let mut isol = isolate(
            IsolationContext::builder(&self.build_appliance)
                // TODO(vmagro): running in a read-only copy of the BA would
                // allow us to skip this snapshot, but that's easier said than
                // done...
                .ephemeral(true)
                .platform([
                    // compiler is built out of the repo, so it needs the
                    // repo to be available
                    repo.as_ref(),
                    #[cfg(facebook)]
                    Path::new("/usr/local/fbcode"),
                    #[cfg(facebook)]
                    Path::new("/mnt/gvfs"),
                ])
                .inputs([
                    // image builds all require the repo for at least the
                    // feature json paths coming from buck
                    repo.as_ref(),
                    self.setup.dnf.repos.as_path(),
                    // layer dependencies require the working volume
                    self.setup.working_dir.as_path(),
                ])
                .working_directory(std::env::current_dir().context("while getting cwd")?)
                // TODO(vmagro): there are currently no tracing args, but
                // there probably should be instead of relying on
                // environment variables...
                .setenv(("RUST_LOG", std::env::var_os("RUST_LOG").unwrap_or_default()))
                .outputs(writable_outputs)
                .build(),
        )
        .into_command();
        isol.arg(std::env::current_exe().context("while getting argv[0]")?);
        if let Some(path) = log_path {
            isol.arg("--append-logs").arg(path);
        }
        match self.subcommand {
            Subcommand::Compile {
                compileish,
                external,
            } => {
                isol.arg("compile").args(
                    Compile {
                        compileish: Compileish {
                            label: self.label,
                            root: subvol.path().to_owned(),
                            external: compileish,
                            dnf: self.setup.dnf,
                            #[cfg(facebook)]
                            fbpkg: self.setup.fbpkg,
                        },
                        external,
                    }
                    .to_args(),
                );
            }
            Subcommand::Plan {
                compileish,
                external,
            } => {
                isol.arg("plan").args(
                    Plan {
                        compileish: Compileish {
                            label: self.label,
                            root: subvol.path().to_owned(),
                            external: compileish,
                            dnf: self.setup.dnf,
                            #[cfg(facebook)]
                            fbpkg: self.setup.fbpkg,
                        },
                        external,
                    }
                    .to_args(),
                );
            }
        }
        debug!("isolated command: {:?}", isol);
        let res = isol
            .spawn()
            .context("while spawning isolated process")?
            .wait()
            .context("while waiting for isolated process")?;
        if !res.success() {
            return Err(anyhow::anyhow!("isolated command failed: {res}").into());
        } else {
            debug!("map finished, making subvol {subvol:?} readonly");
            subvol
                .set_readonly(true)
                .context("while making subvol r/o")?;
            debug!(
                "linking {} -> {}",
                self.setup.output.display(),
                subvol.path().display(),
            );
            let _ = std::fs::remove_file(&self.setup.output);
            std::os::unix::fs::symlink(subvol.path(), &self.setup.output)
                .context("while making symlink")?;
            Ok(())
        }
    }
}
