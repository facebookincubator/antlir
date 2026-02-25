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
pub(crate) struct Parse {
    #[clap(subcommand)]
    sub: Sub,
}

#[derive(Debug, Subcommand)]
enum Sub {
    /// Parse metadata from deb repos
    Deb(deb::Parse),
}

impl Parse {
    pub(crate) fn run(&self) -> Result<()> {
        match &self.sub {
            Sub::Deb(sub) => sub.run(),
        }
    }
}
