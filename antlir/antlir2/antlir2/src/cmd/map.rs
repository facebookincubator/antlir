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

impl Map {
    /// Create a new mutable subvolume based on the [SetupArgs].
    #[tracing::instrument(skip(self, rootless), ret, err)]
    fn create_new_subvol(
        &self,
        working_volume: &WorkingVolume,
        rootless: antlir2_rootless::Rootless,
    ) -> Result<Subvolume> {
        let dst = working_volume
            .allocate_new_path()
            .context("while allocating new path for subvol")?;
        let _guard = rootless.escalate()?;
        let subvol = match &self.setup.parent {
            Some(parent) => {
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
        let mut subvol = self.create_new_subvol(&working_volume, rootless)?;

        // Be careful to isolate this process from the host mount namespace in
        // case anything weird is going on
        rootless.as_root(|| {
            unshare(CloneFlags::CLONE_NEWNS)?;

            // Remount / as private so that we don't let any changes escape back
            // to the parent mount namespace (basically equivalent to `mount
            // --make-rprivate /`)
            mount(
                None::<&str>,
                "/",
                None::<&str>,
                MsFlags::MS_REC | MsFlags::MS_PRIVATE,
                None::<&str>,
            )?;
            Ok::<_, anyhow::Error>(())
        })??;

        // TODO: don't do this as root, only escalate when actually necessary
        // Running the whole subcommand as root matches the behavior we got when
        // this was run inside the build appliance container.
        // Soon this will run in a user namespace and won't need "real"
        // privilege escalation.
        rootless.as_root(|| match self.subcommand {
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
            .run(),
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
            .run(),
        })??;
        debug!("map finished, making subvol {subvol:?} readonly");
        rootless.as_root(|| subvol.set_readonly(true).context("while making subvol r/o"))??;
        debug!(
            "linking {} -> {}",
            self.setup.output.display(),
            subvol.path().display(),
        );
        let _ = std::fs::remove_file(&self.setup.output);
        std::os::unix::fs::symlink(subvol.path(), &self.setup.output)
            .context("while making symlink")?;

        rootless.as_root(|| {
            working_volume
                .keep_path_alive(subvol.path(), &self.setup.output)
                .context("while setting up refcount")?;
            working_volume
                .collect_garbage()
                .context("while garbage collecting old outputs")?;
            Ok::<_, anyhow::Error>(())
        })??;

        Ok(())
    }
}
