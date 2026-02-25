/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;

mod deb;

#[derive(Debug, Parser)]
pub(crate) struct Generate {
    #[clap(subcommand)]
    sub: Sub,
}

#[derive(Debug, Subcommand)]
enum Sub {
    /// Generate metadata for deb repos
    Deb(deb::Generate),
}

impl Generate {
    pub(crate) fn run(self) -> Result<()> {
        match self.sub {
            Sub::Deb(sub) => sub.run(),
        }
    }
}
