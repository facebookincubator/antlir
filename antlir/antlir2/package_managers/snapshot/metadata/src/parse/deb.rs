/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;

mod packages;
mod release;

#[derive(Debug, Parser)]
pub(crate) struct Parse {
    #[clap(subcommand)]
    sub: Sub,
}

#[derive(Debug, Subcommand)]
enum Sub {
    /// Parse a Debian Release file and return information required to download
    /// all the other metadata files.
    Release(release::ParseRelease),
    /// Parse a Debian Packages index file into structured JSON.
    Packages(packages::ParsePackages),
}

impl Parse {
    pub(crate) fn run(&self) -> Result<()> {
        match &self.sub {
            Sub::Release(sub) => sub.run(),
            Sub::Packages(sub) => sub.run(),
        }
    }
}
