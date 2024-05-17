/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use antlir2_btrfs::Subvolume;
use antlir2_working_volume::WorkingVolume;
use anyhow::Context;
use buck_label::Label;
use clap::Parser;
use nix::mount::mount;
use nix::mount::MsFlags;
use nix::sched::unshare;
use nix::sched::CloneFlags;
use tracing::debug;
use tracing::trace;
use tracing::warn;

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
    build_appliance: PathBuf,
    #[clap(long)]
    /// Use an unprivileged usernamespace
    rootless: bool,
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

impl Map {
    /// Create a new mutable subvolume based on the [SetupArgs].
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
        let subvol = match &self.setup.parent {
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

    #[tracing::instrument(name = "map", skip_all, ret, err)]
    pub(crate) fn run(self, rootless: antlir2_rootless::Rootless) -> Result<()> {
        let working_volume = WorkingVolume::ensure(self.setup.working_dir.clone())
            .context("while setting up WorkingVolume")?;
        let rootless = match self.rootless {
            true => None,
            false => Some(rootless),
        };

        if self.rootless {
            antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
        }

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

        match self.subcommand {
            Subcommand::Compile {
                compileish,
                external,
            } => Compile {
                compileish: Compileish {
                    label: self.label,
                    root: subvol.path().to_owned(),
                    build_appliance: self.build_appliance,
                    external: compileish,
                    dnf: self.setup.dnf,
                    #[cfg(facebook)]
                    fbpkg: self.setup.fbpkg,
                },
                external,
            }
            .run(rootless),
            Subcommand::Plan {
                compileish,
                external,
            } => Plan {
                compileish: Compileish {
                    label: self.label,
                    root: subvol.path().to_owned(),
                    build_appliance: self.build_appliance,
                    external: compileish,
                    dnf: self.setup.dnf,
                    #[cfg(facebook)]
                    fbpkg: self.setup.fbpkg,
                },
                external,
            }
            .run(rootless),
        }?;
        debug!("map finished, making subvol {subvol:?} readonly");

        let root_guard = rootless.map(|r| r.escalate()).transpose()?;

        if self.setup.output.exists() {
            trace!("removing existing output {}", self.setup.output.display());
            // Don't fail if the old subvol couldn't be deleted, just print
            // a warning. We really don't want to fail someone's build if
            // the only thing that went wrong is not being able to delete
            // the last version of it.
            match Subvolume::open(&self.setup.output) {
                Ok(old_subvol) => {
                    if let Err(e) = old_subvol.delete() {
                        warn!(
                            "couldn't delete old subvol '{}': {e:?}",
                            self.setup.output.display()
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        "couldn't open old subvol '{}': {e:?}",
                        self.setup.output.display()
                    );
                }
            }
        }

        subvol
            .set_readonly(true)
            .context("while making subvol r/o")?;

        debug!(
            "linking {} -> {}",
            self.setup.output.display(),
            subvol.path().display(),
        );
        drop(root_guard);

        let _ = std::fs::remove_file(&self.setup.output);
        std::os::unix::fs::symlink(subvol.path(), &self.setup.output)
            .context("while making symlink")?;

        Ok(())
    }
}
