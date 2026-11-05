/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Write as _;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;

/// Write checksums for every regular file below a directory to JSON.
///
/// Used as a Buck action by `snapshot.bxl` so the snapshot binary only
/// needs to compare two of these JSON files to decide whether a
/// metadata tree changed.
#[derive(Parser, Debug)]
pub(crate) struct DirectoryChecksums {
    #[clap(long)]
    tree: PathBuf,
    #[clap(long)]
    out: PathBuf,
}

impl DirectoryChecksums {
    pub(crate) fn run(self) -> Result<()> {
        let checksums = snapshot_common::directory_checksums(&self.tree)
            .with_context(|| format!("failed to checksum {}", self.tree.display()))?;
        let mut outfile = std::io::BufWriter::new(
            stdio_path::create(&self.out)
                .with_context(|| format!("failed to create {}", self.out.display()))?,
        );
        serde_json::to_writer(&mut outfile, &checksums).context("failed to write checksums")?;
        outfile.flush().context("failed to flush checksums")?;
        Ok(())
    }
}
