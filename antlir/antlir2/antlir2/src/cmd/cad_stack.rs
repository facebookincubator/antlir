/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use antlir2_btrfs::Subvolume;
use antlir2_rootless::Rootless;
use antlir2_working_volume::WorkingVolume;
use anyhow::Context;
use cad_stack_fs::extract_root_dir;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use clap::Parser;
use clap::Subcommand;

use crate::Result;

#[derive(Subcommand, Debug)]
pub(crate) enum CadStack {
    Materialize(Materialize),
}

impl CadStack {
    pub(crate) fn run(self, rootless: Rootless) -> Result<()> {
        match self {
            CadStack::Materialize(cmd) => cmd.run(rootless),
        }
    }
}

#[derive(Parser, Debug)]
pub(crate) struct Materialize {
    #[clap(long)]
    top: PathBuf,
    #[clap(long)]
    parent: Vec<PathBuf>,
    #[clap(long)]
    rootless: bool,
    #[clap(long)]
    subvol_symlink: PathBuf,
}

impl Materialize {
    #[tracing::instrument(name = "materialize", skip(self))]
    pub(crate) fn run(self, rootless: Rootless) -> Result<()> {
        let rootless = match self.rootless {
            true => None,
            false => Some(rootless),
        };

        if self.rootless {
            antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
        }

        let root_guard = rootless.map(|r| r.escalate()).transpose()?;

        let stack = cad_stack::ObjectStore::open_rw(
            self.top.join("store"),
            self.parent.iter().map(|p| p.join("store")),
        )
        .context("while opening stack")?;

        let wv = WorkingVolume::ensure()?;
        let dst = wv
            .allocate_new_subvol_path()
            .context("while allocating new path for subvol")?;
        let subvol = Subvolume::create(&dst).context("while creating empty subvolume")?;

        extract_root_dir(
            &stack,
            &Dir::open_ambient_dir(subvol.path(), ambient_authority())
                .context("while opening new root")?,
        )
        .context("while re-hydrating stack")?;

        drop(root_guard);

        std::os::unix::fs::symlink(subvol.path(), &self.subvol_symlink)
            .context("while making symlink")?;

        Ok(())
    }
}
