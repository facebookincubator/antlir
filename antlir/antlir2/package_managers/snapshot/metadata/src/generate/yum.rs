/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;

mod repomd;

#[derive(Debug, Parser)]
pub(crate) struct Generate {
    #[clap(subcommand)]
    sub: Sub,
}

#[derive(Debug, Subcommand)]
enum Sub {
    /// Generate a canonical repomd.xml referencing the decompressed
    /// repodata files with both sha1 and sha256 checksums.
    Repomd(repomd::GenerateRepomd),
}

impl Generate {
    pub(crate) fn run(self) -> Result<()> {
        match self.sub {
            Sub::Repomd(sub) => sub.run(),
        }
    }
}
