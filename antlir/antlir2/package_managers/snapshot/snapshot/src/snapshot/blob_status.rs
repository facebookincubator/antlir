/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use clap::Parser;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use json_arg::JsonFile;
use serde::Deserialize;
use serde::Serialize;
use snapshot_common::Checksums;
use tracing::debug;

use super::storage::BlobStatus;
use super::storage::Storage;

const MAX_CONCURRENT_CHECKS: usize = 1024;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Entry {
    pub(crate) filename: String,
    pub(crate) checksums: Checksums,
    #[serde(flatten)]
    rest: serde_json::Value,
}

/// The result of checking blob status in storage.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BlobStatusOutput {
    /// Blobs that do not exist in storage and need to be downloaded and uploaded.
    pub(crate) missing: Vec<Entry>,
    /// Blobs that exist but will expire within the threshold and need TTL extension.
    pub(crate) expiring_soon: Vec<Entry>,
}

#[derive(Parser, Debug)]
pub(crate) struct CheckBlobStatus {
    /// Path(s) to JSON files containing lists of blob entries to check.  Each
    /// file should be a JSON array of objects with a "checksums" field.
    #[clap(long)]
    entries: Vec<JsonFile<Vec<Entry>>>,
}

impl CheckBlobStatus {
    #[tracing::instrument(skip(self, storage), ret, err)]
    pub(crate) async fn run(self, storage: Box<dyn Storage>) -> Result<BlobStatusOutput> {
        let results: Vec<_> =
            stream::iter(self.entries.into_iter().flat_map(JsonFile::into_inner).map(
                |entry| async {
                    let status = storage.blob_status(&entry.checksums).await?;
                    debug!(filename = entry.filename, ?status, "blob status");
                    Ok::<_, anyhow::Error>((entry, status))
                },
            ))
            .buffer_unordered(MAX_CONCURRENT_CHECKS)
            .try_collect()
            .await?;

        let mut missing = Vec::new();
        let mut expiring_soon = Vec::new();
        for (entry, status) in results {
            match status {
                BlobStatus::Missing => missing.push(entry),
                BlobStatus::ExpiringSoon => expiring_soon.push(entry),
                BlobStatus::Fresh => {}
            }
        }

        Ok(BlobStatusOutput {
            missing,
            expiring_soon,
        })
    }
}
