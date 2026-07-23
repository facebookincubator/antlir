/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::io::BufWriter;
use std::io::Write as _;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use json_arg::Json;
use json_arg::JsonFile;
use serde::Deserialize;
use serde::Serialize;
use snapshot_common::Checksums;
use tracing::debug;

use super::progress;
use super::storage::BlobStatus;
use super::storage::Storage;
use super::storage::StorageConfig;

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
    /// Blobs that exist with fresh TTL. Emitted for observability in the
    /// serialized status output; the in-process pipeline does not consume it
    /// (index building reads `full_checksums` instead).
    #[serde(default)]
    pub(crate) fresh: Vec<Entry>,
    /// Full checksums (sha1+sha256) fetched from flat object properties during
    /// the status check. Mapping from filename to full checksums. This avoids
    /// a separate `get_file_checksums` RPC per file in the index phase.
    #[serde(default)]
    pub(crate) full_checksums: BTreeMap<String, Checksums>,
}

#[derive(Parser, Debug)]
pub(crate) struct CheckBlobStatus {
    #[clap(long)]
    out: PathBuf,
    #[clap(long)]
    storage: Json<StorageConfig>,
    /// Path(s) to JSON files containing lists of blob entries to check.  Each
    /// file should be a JSON array of objects with a "checksums" field.
    #[clap(long)]
    entries: Vec<JsonFile<Vec<Entry>>>,
}

impl CheckBlobStatus {
    #[tracing::instrument(skip(self, fb), ret, err)]
    pub(crate) async fn run(self, fb: fbinit::FacebookInit) -> Result<()> {
        let storage = self.storage.into_inner().build(fb)?;
        let entries: Vec<Entry> = self
            .entries
            .into_iter()
            .flat_map(JsonFile::into_inner)
            .collect();
        let out = check_blob_status(&*storage, entries).await?;
        let mut outfile = BufWriter::new(stdio_path::create(&self.out)?);
        serde_json::to_writer(&mut outfile, &out)?;
        // BufWriter swallows flush errors on drop; flush explicitly so a
        // disk-full / EIO surfaces instead of silently leaving a partial JSON.
        outfile
            .flush()
            .context("failed to flush blob-status output")?;
        Ok(())
    }
}

pub(crate) async fn check_blob_status(
    storage: &dyn Storage,
    entries: Vec<Entry>,
) -> Result<BlobStatusOutput> {
    // Deduplicate entries by checksums to avoid checking same blob multiple times
    // Keep first filename for each checksum key for index mapping.
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for e in entries {
        // Use sha256 as dedup key if present, else sha1, else filename
        let dedup_key = if let Some(sha256) = &e.checksums.sha256 {
            format!("sha256:{}", hex::encode(sha256))
        } else if let Some(sha1) = &e.checksums.sha1 {
            format!("sha1:{}", hex::encode(sha1))
        } else {
            e.filename.clone()
        };
        if seen.insert(dedup_key) {
            deduped.push(e);
        } else {
            debug!(filename = e.filename, "skipping duplicate blob entry");
        }
    }

    let total_entries = deduped.len();
    if total_entries == 0 {
        return Ok(BlobStatusOutput {
            missing: Vec::new(),
            expiring_soon: Vec::new(),
            fresh: Vec::new(),
            full_checksums: BTreeMap::new(),
        });
    }

    let pb = progress::bar(total_entries, "Checking blob status");
    let results: Vec<_> = stream::iter(deduped.into_iter().map(|entry| {
        let pb = pb.clone();
        async move {
            let (status, full) = storage
                .flat_status_and_full_checksums(&entry.checksums)
                .await?;
            debug!(filename = entry.filename, ?status, ?full, "blob status");
            pb.inc(1);
            Ok::<_, anyhow::Error>((entry, status, full))
        }
    }))
    .buffer_unordered(MAX_CONCURRENT_CHECKS)
    .try_collect()
    .await?;
    pb.finish_with_message("Checked blob status");

    let mut missing = Vec::new();
    let mut expiring_soon = Vec::new();
    let mut fresh = Vec::new();
    let mut full_checksums = BTreeMap::new();

    for (entry, status, full) in results {
        if let Some(f) = full {
            // Store full checksums from flat properties for reuse in index.
            // Duplicate filenames last-wins but now deduplicated above, so this is safe. Log if collision.
            if full_checksums.contains_key(&entry.filename) {
                debug!(
                    filename = entry.filename,
                    "duplicate filename in full_checksums, overwriting"
                );
            }
            full_checksums.insert(entry.filename.clone(), f);
        }
        match status {
            BlobStatus::Missing => missing.push(entry),
            BlobStatus::ExpiringSoon => expiring_soon.push(entry),
            BlobStatus::Fresh => fresh.push(entry),
        }
    }

    Ok(BlobStatusOutput {
        missing,
        expiring_soon,
        fresh,
        full_checksums,
    })
}
