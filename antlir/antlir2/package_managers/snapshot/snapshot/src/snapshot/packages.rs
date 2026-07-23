/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::io::BufWriter;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use backoff::ExponentialBackoff;
use backoff::ExponentialBackoffBuilder;
use clap::Parser;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use json_arg::Json;
use json_arg::JsonFile;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::Out;
use super::blob_status::BlobStatusOutput;
use super::blob_status::Entry;
use super::progress;
use super::storage::BlobStatus;
use super::storage::Storage;
use super::storage::StorageConfig;
use super::storage::UrlWithChecksums;

pub(crate) const MAX_CONCURRENT: usize = 100;
pub(crate) const MAX_CONCURRENT_SYMLINK: usize = 500;
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);
const MAX_ELAPSED: Duration = Duration::from_secs(60);

/// Exponential-backoff policy (with built-in jitter) for retrying transient
/// network operations against the storage backend and upstream repos.
pub(crate) fn retry_policy() -> ExponentialBackoff {
    ExponentialBackoffBuilder::new()
        .with_initial_interval(INITIAL_BACKOFF)
        .with_max_elapsed_time(Some(MAX_ELAPSED))
        .build()
}

#[derive(Parser, Debug)]
pub(crate) struct Packages {
    #[clap(long)]
    out: PathBuf,
    #[clap(long)]
    storage: Json<StorageConfig>,
    /// JSON file containing the output of blob-status (missing + expiring_soon).
    #[clap(long)]
    blob_status: JsonFile<BlobStatusOutput>,
    /// Path(s) to JSON files containing the full list of blob entries. After
    /// uploading missing blobs, every entry here gets a tree symlink pointing
    /// at its flat content-addressed object.
    #[clap(long)]
    all_entries: Vec<JsonFile<Vec<Entry>>>,
    /// Base URL to download packages from.
    #[clap(long)]
    base_url: String,
}

/// Download a file from `url` to a new temp file. Returns the NamedTempFile
/// handle (which auto-deletes on drop) and its path.
async fn download(client: &reqwest::Client, url: &str) -> Result<(NamedTempFile, PathBuf)> {
    let mut response = client
        .get(url)
        .send()
        .await
        .with_context(|| format!("failed to request {url}"))?
        .error_for_status()
        .with_context(|| format!("HTTP error downloading {url}"))?;

    // NamedTempFile::new() does blocking filesystem I/O, so run it in spawn_blocking.
    let tmp = tokio::task::spawn_blocking(NamedTempFile::new)
        .await
        .context("spawn_blocking failed")?
        .context("failed to create temp file")?;
    let tmp_path = tmp.path().to_owned();

    let mut file = tokio::fs::File::create(&tmp_path)
        .await
        .context("failed to open temp file for writing")?;
    while let Some(chunk) = response
        .chunk()
        .await
        .with_context(|| format!("failed to read chunk from {url}"))?
    {
        file.write_all(&chunk)
            .await
            .context("failed to write chunk to temp file")?;
    }
    file.flush().await?;

    Ok((tmp, tmp_path))
}

/// Upload a file as a flat content-addressed object only, reusing checksums
/// the caller already computed to avoid re-hashing the file.
async fn upload_flat(
    storage: &dyn Storage,
    path: &Path,
    checksums: &snapshot_common::Checksums,
) -> Result<UrlWithChecksums> {
    storage
        .store_flat_with_checksums(path, checksums)
        .await
        .with_context(|| format!("failed to upload flat {}", path.display()))
}

/// Download a package, verify its checksum against expected, then upload flat with retries.
async fn download_and_upload(
    client: &reqwest::Client,
    storage: &dyn Storage,
    download_url: &str,
    filename: &str,
    expected_checksums: &snapshot_common::Checksums,
) -> Result<UrlWithChecksums> {
    let file_start = std::time::Instant::now();

    // Download + verify as one retry unit: a corrupt download is recovered by
    // re-downloading. Every failure here is transient and retried by the policy.
    let (tmp, tmp_path, computed_checksums) = backoff::future::retry_notify(
        retry_policy(),
        || async {
            let dl_start = std::time::Instant::now();
            let (tmp, tmp_path) = download(client, download_url)
                .await
                .map_err(backoff::Error::transient)?;
            let dl_elapsed = dl_start.elapsed();
            if dl_elapsed > Duration::from_secs(30) {
                warn!(filename, ?dl_elapsed, "slow download");
            }

            // Verify checksums before upload (supply-chain integrity).
            let computed = snapshot_common::Checksums::from_file_async(tmp_path.clone())
                .await
                .map_err(backoff::Error::transient)?;
            expected_checksums
                .verify_against(&computed)
                .map_err(|e| backoff::Error::transient(anyhow::Error::from(e)))?;

            Ok::<_, backoff::Error<anyhow::Error>>((tmp, tmp_path, computed))
        },
        |e, dur| warn!(filename, error = %e, ?dur, "retrying download after failure"),
    )
    .await?;

    // Upload as a separate retry unit so a flaky upload doesn't force us to
    // re-download the (already verified) file.
    let result = backoff::future::retry_notify(
        retry_policy(),
        || async {
            let up_start = std::time::Instant::now();
            let result = upload_flat(storage, &tmp_path, &computed_checksums)
                .await
                .map_err(backoff::Error::transient)?;
            let up_elapsed = up_start.elapsed();
            if up_elapsed > Duration::from_secs(30) {
                warn!(filename, ?up_elapsed, "slow upload");
            }
            Ok::<_, backoff::Error<anyhow::Error>>(result)
        },
        |e, dur| warn!(filename, error = %e, ?dur, "retrying upload after failure"),
    )
    .await?;

    debug!(
        filename,
        total_elapsed = ?file_start.elapsed(),
        "download+upload complete"
    );
    drop(tmp);
    Ok(result)
}

impl Packages {
    #[tracing::instrument(skip(self, fb), ret, err)]
    pub(crate) async fn run(self, fb: fbinit::FacebookInit) -> Result<()> {
        let storage = self.storage.into_inner().build(fb)?;
        let storage: std::sync::Arc<dyn Storage> = storage.into();
        let blob_status = self.blob_status.into_inner();
        let all_entries: Vec<Entry> = self
            .all_entries
            .into_iter()
            .flat_map(JsonFile::into_inner)
            .collect();
        let out = snapshot_packages(storage, blob_status, all_entries, &self.base_url).await?;
        let mut outfile = BufWriter::new(stdio_path::create(&self.out)?);
        serde_json::to_writer(&mut outfile, &out)?;
        // BufWriter swallows flush errors on drop; flush explicitly so a
        // disk-full / EIO surfaces instead of silently leaving a partial JSON.
        outfile.flush().context("failed to flush packages output")?;
        Ok(())
    }
}

pub(crate) async fn snapshot_packages(
    storage: std::sync::Arc<dyn Storage>,
    blob_status: BlobStatusOutput,
    all_entries: Vec<Entry>,
    base_url: &str,
) -> Result<Out> {
    let base_url = Arc::new(base_url.to_owned());

    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(300))
        .connect_timeout(Duration::from_secs(30))
        .pool_max_idle_per_host(MAX_CONCURRENT)
        .pool_idle_timeout(Duration::from_secs(30))
        .build()
        .context("failed to build HTTP client")?;
    let client = Arc::new(client);

    // Extend TTLs for blobs that exist but are expiring soon
    let expiring_soon_len = blob_status.expiring_soon.len();
    info!("extending TTL for {expiring_soon_len} expiring-soon blobs");
    let expiring_pb = progress::bar(expiring_soon_len, "Extending TTL");
    stream::iter(blob_status.expiring_soon.into_iter().map(|entry| {
        let storage = Arc::clone(&storage);
        let pb = expiring_pb.clone();
        async move {
            debug!(filename = entry.filename, "extending TTL for expiring blob");
            let res = storage
                .extend_ttl(&entry.checksums)
                .await
                .with_context(|| format!("failed to extend TTL for {}", entry.filename));
            pb.inc(1);
            res?;
            Ok::<_, anyhow::Error>(())
        }
    }))
    .buffer_unordered(MAX_CONCURRENT)
    .try_collect::<Vec<_>>()
    .await?;
    expiring_pb.finish_with_message("Extended TTL");

    let entries = blob_status.missing;
    info!("downloading and uploading {} package blobs", entries.len());
    let download_pb = progress::bar(entries.len(), "Downloading & uploading packages");

    let results: Vec<_> = stream::iter(entries.into_iter().map(|entry| {
        let storage = Arc::clone(&storage);
        let client = Arc::clone(&client);
        let base_url = Arc::clone(&base_url);
        let pb = download_pb.clone();
        async move {
            // Check if the blob already exists in storage (race condition:
            // another process may have uploaded it between blob-status and
            // now). If it does, just extend its TTL instead of downloading.
            let status = storage.blob_status(&entry.checksums).await?;
            match status {
                BlobStatus::Fresh | BlobStatus::ExpiringSoon => {
                    info!(
                        filename = entry.filename,
                        "blob already exists ({status:?}), extending its ttl",
                    );
                    storage.extend_ttl(&entry.checksums).await?;
                    pb.inc(1);
                    return Ok::<_, anyhow::Error>(None);
                }
                BlobStatus::Missing => {}
            }

            let download_url = format!("{}/{}", base_url, entry.filename);
            let tree_key = entry.filename.clone();
            debug!(
                filename = entry.filename,
                download_url, tree_key, "downloading package blob"
            );

            let result = download_and_upload(
                &client,
                &*storage,
                &download_url,
                &tree_key,
                &entry.checksums,
            )
            .await?;
            pb.inc(1);

            Ok(Some((entry.filename, result)))
        }
    }))
    .buffer_unordered(MAX_CONCURRENT)
    .try_collect()
    .await?;
    download_pb.finish_with_message("Downloaded packages");

    let mut files = BTreeMap::new();
    let mut checksums = BTreeMap::new();
    for result in results {
        if let Some((key, uwc)) = result {
            let url = uwc.url.to_string();
            checksums.insert(url.clone(), uwc.checksums);
            files.insert(key, url);
        }
    }

    // (Re)create tree symlinks for every known blob so the tree namespace
    // is complete and has a fresh TTL.
    info!("symlinking {} blobs into tree namespace", all_entries.len());

    // Pre-create parent directories in bulk with bounded concurrency
    let tree_keys_for_dirs: Vec<String> = all_entries.iter().map(|e| e.filename.clone()).collect();
    storage
        .ensure_tree_dirs(&tree_keys_for_dirs)
        .await
        .context("failed to pre-create tree parent directories")?;

    let symlink_pb = progress::bar(all_entries.len(), "Linking packages into tree");
    stream::iter(all_entries.into_iter().map(|entry| {
        let storage = Arc::clone(&storage);
        let pb = symlink_pb.clone();
        async move {
            let tree_key = entry.filename.clone();
            let res = storage
                .symlink_flat_to_tree(&entry.checksums, &tree_key)
                .await
                .with_context(|| format!("failed to symlink {} into tree", entry.filename));
            pb.inc(1);
            res
        }
    }))
    .buffer_unordered(MAX_CONCURRENT_SYMLINK)
    .try_collect::<Vec<_>>()
    .await?;
    symlink_pb.finish_with_message("Linked packages");

    Ok(Out { files, checksums })
}
