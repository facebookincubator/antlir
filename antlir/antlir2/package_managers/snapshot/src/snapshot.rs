/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::io::BufWriter;
use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use clap::Subcommand;
use json_arg::Json;
use serde::Serialize;

use crate::checksums::Checksums;

mod metadata;
mod storage;
use storage::StorageConfig;

#[derive(Debug, Parser)]
pub(crate) struct Snapshot {
    #[clap(subcommand)]
    sub: Sub,
    #[clap(long)]
    out: PathBuf,
    #[clap(long)]
    storage: Json<StorageConfig>,
}

#[derive(Debug, Serialize)]
pub(super) struct Out {
    pub(super) files: BTreeMap<String, String>,
    pub(super) checksums: BTreeMap<String, Checksums>,
}

#[derive(Debug, Subcommand)]
enum Sub {
    /// Snapshot metadata tree
    Metadata(metadata::Metadata),
}

impl Snapshot {
    pub(crate) async fn run(self, fb: fbinit::FacebookInit) -> Result<()> {
        let storage = self.storage.into_inner().build(fb)?;
        let out = match &self.sub {
            Sub::Metadata(metadata) => metadata.run(storage).await,
        }?;
        let outfile = BufWriter::new(stdio_path::create(&self.out)?);
        serde_json::to_writer(outfile, &out)?;
        Ok(())
    }
}
