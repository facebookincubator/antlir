/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use antlir2_cas_dir::CasDirOpts;
use anyhow::Context;
use clap::Parser;
use clap::Subcommand;

use crate::Result;

#[derive(Parser, Debug)]
pub(crate) struct CasDir {
    #[clap(long)]
    rootless: bool,
    #[command(subcommand)]
    sub: Sub,
}

#[derive(Subcommand, Debug)]
enum Sub {
    Dehydrate(Dehydrate),
}

#[derive(Parser, Debug)]
struct Dehydrate {
    #[clap(long)]
    subvol: PathBuf,
    #[clap(long)]
    out: PathBuf,
}

impl CasDir {
    #[tracing::instrument(name = "cas-dir", skip_all, ret, err)]
    pub(crate) fn run(self, rootless: antlir2_rootless::Rootless) -> Result<()> {
        // This naming is a little confusing, but basically `rootless` exists to
        // drop privileges when the process is invoked with `sudo`, and as such
        // is not used if the entire build is running solely as an unprivileged
        // user.
        let rootless = if self.rootless {
            antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
            None
        } else {
            Some(rootless)
        };
        match self.sub {
            Sub::Dehydrate(dehydrate) => {
                let mut opts = CasDirOpts::default();
                if let Some(uid) = rootless.and_then(|r| r.unprivileged_uid()) {
                    opts = opts.uid(uid);
                }
                if let Some(gid) = rootless.and_then(|r| r.unprivileged_gid()) {
                    opts = opts.gid(gid);
                }
                let root_guard = rootless.map(|r| r.escalate()).transpose()?;
                antlir2_cas_dir::CasDir::dehydrate(&dehydrate.subvol, dehydrate.out, opts)?;
                drop(root_guard);
            }
        }
        Ok(())
    }
}
