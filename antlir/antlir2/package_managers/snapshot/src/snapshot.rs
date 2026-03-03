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

mod blob_status;
mod metadata;
mod packages;
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
    /// Check blob status in storage (missing or expiring soon)
    BlobStatus(blob_status::CheckBlobStatus),
    /// Download missing packages and upload them to storage
    Packages(packages::Packages),
}

impl Snapshot {
    pub(crate) async fn run(self, fb: fbinit::FacebookInit) -> Result<()> {
        let storage = self.storage.into_inner().build(fb)?;
        let outfile = BufWriter::new(stdio_path::create(&self.out)?);
        match self.sub {
            Sub::Metadata(metadata) => {
                let out = metadata.run(storage).await?;
                serde_json::to_writer(outfile, &out)?;
            }
            Sub::BlobStatus(blob_status) => {
                let out = blob_status.run(storage).await?;
                serde_json::to_writer(outfile, &out)?;
            }
            Sub::Packages(packages) => {
                let out = packages.run(storage).await?;
                serde_json::to_writer(outfile, &out)?;
            }
        }
        Ok(())
    }
}
