/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;

mod release;

#[derive(Debug, Parser)]
pub(crate) struct Generate {
    #[clap(subcommand)]
    sub: Sub,
}

#[derive(Debug, Subcommand)]
enum Sub {
    /// Generate an unsigned Release file from a release.json and component
    /// Packages files.
    Release(release::GenerateRelease),
}

impl Generate {
    pub(crate) fn run(self) -> Result<()> {
        match self.sub {
            Sub::Release(sub) => sub.run(),
        }
    }
}
