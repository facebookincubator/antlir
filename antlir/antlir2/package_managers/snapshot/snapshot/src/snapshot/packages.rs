/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use json_arg::JsonFile;
use tempfile::NamedTempFile;
use tokio::io::AsyncWriteExt;
use tracing::debug;
use tracing::info;
use tracing::warn;

use super::Out;
use super::blob_status::BlobStatusOutput;
use super::blob_status::Entry;
use super::storage::BlobStatus;
use super::storage::Storage;
use super::storage::UrlWithChecksums;

const MAX_CONCURRENT: usize = 100;
const MAX_RETRIES: usize = 5;
const INITIAL_BACKOFF: Duration = Duration::from_secs(1);

#[derive(Parser, Debug)]
pub(crate) struct Packages {
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

    let tmp = tokio::task::spawn_blocking(NamedTempFile::new)
        .await?
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

/// Upload a file from `path` to storage, creating both a flat content-addressed
/// object and a tree symlink at `filename`.
async fn upload(storage: &dyn Storage, path: &Path, filename: &str) -> Result<UrlWithChecksums> {
    storage
        .store(path, filename)
        .await
        .with_context(|| format!("failed to upload {filename}"))
}

/// Download a package and upload it to storage, with progressive retries.
/// If the download fails, the whole operation is retried. If the upload fails,
/// only the upload is retried from the already-downloaded file.
async fn download_and_upload(
    client: &reqwest::Client,
    storage: &dyn Storage,
    download_url: &str,
    filename: &str,
) -> Result<UrlWithChecksums> {
    let mut last_err: Option<anyhow::Error> = None;

    for attempt in 0..MAX_RETRIES {
        if attempt > 0 {
            let backoff = INITIAL_BACKOFF * 2u32.pow(attempt as u32 - 1);
            warn!(
                filename,
                attempt,
                ?backoff,
                "retrying download after failure"
            );
            tokio::time::sleep(backoff).await;
        }

        let (tmp, tmp_path) = match download(client, download_url).await {
            Ok(result) => result,
            Err(e) => {
                warn!(filename, attempt, error = %e, "download failed");
                last_err = Some(e);
                continue;
            }
        };

        // Upload with retries (reusing the downloaded file)
        let mut upload_err: Option<anyhow::Error> = None;
        for upload_attempt in 0..MAX_RETRIES {
            if upload_attempt > 0 {
                let backoff = INITIAL_BACKOFF * 2u32.pow(upload_attempt as u32 - 1);
                warn!(
                    filename,
                    upload_attempt,
                    ?backoff,
                    "retrying upload after failure"
                );
                tokio::time::sleep(backoff).await;
            }

            match upload(storage, &tmp_path, filename).await {
                Ok(result) => {
                    drop(tmp);
                    return Ok(result);
                }
                Err(e) => {
                    warn!(filename, upload_attempt, error = %e, "upload failed");
                    upload_err = Some(e);
                }
            }
        }

        // All upload retries exhausted — re-download and try again
        drop(tmp);
        last_err = upload_err;
    }

    Err(last_err.unwrap_or_else(|| anyhow::anyhow!("download_and_upload failed for {filename}")))
}

impl Packages {
    #[tracing::instrument(skip(self, storage), ret, err)]
    pub(crate) async fn run(self, storage: Box<dyn Storage>) -> Result<Out> {
        let storage: Arc<dyn Storage> = Arc::from(storage);
        let blob_status = self.blob_status.into_inner();
        let all_entries: Vec<Entry> = self
            .all_entries
            .into_iter()
            .flat_map(JsonFile::into_inner)
            .collect();
        let base_url = Arc::new(self.base_url);

        let client = reqwest::Client::builder()
            .build()
            .context("failed to build HTTP client")?;
        let client = Arc::new(client);

        // Extend TTLs for blobs that exist but are expiring soon
        info!(
            "extending TTL for {} expiring-soon blobs",
            blob_status.expiring_soon.len()
        );
        stream::iter(blob_status.expiring_soon.into_iter().map(|entry| {
            let storage = Arc::clone(&storage);
            async move {
                debug!(filename = entry.filename, "extending TTL for expiring blob");
                storage
                    .extend_ttl(&entry.checksums)
                    .await
                    .with_context(|| format!("failed to extend TTL for {}", entry.filename))?;
                Ok::<_, anyhow::Error>(())
            }
        }))
        .buffer_unordered(MAX_CONCURRENT)
        .try_collect::<Vec<_>>()
        .await?;

        let entries = blob_status.missing;
        info!("downloading and uploading {} package blobs", entries.len());

        let results: Vec<_> = stream::iter(entries.into_iter().map(|entry| {
            let storage = Arc::clone(&storage);
            let client = Arc::clone(&client);
            let base_url = Arc::clone(&base_url);
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
                        return Ok::<_, anyhow::Error>(None);
                    }
                    BlobStatus::Missing => {}
                }

                let download_url = format!("{}/{}", base_url, entry.filename);
                debug!(
                    filename = entry.filename,
                    download_url, "downloading package blob"
                );

                let result =
                    download_and_upload(&client, &*storage, &download_url, &entry.filename).await?;

                Ok(Some((entry.filename, result)))
            }
        }))
        .buffer_unordered(MAX_CONCURRENT)
        .try_collect()
        .await?;

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
        stream::iter(all_entries.into_iter().map(|entry| {
            let storage = Arc::clone(&storage);
            async move {
                storage
                    .symlink_flat_to_tree(&entry.checksums, &format!("archive/{}", &entry.filename))
                    .await
                    .with_context(|| format!("failed to symlink {} into tree", entry.filename))
            }
        }))
        .buffer_unordered(MAX_CONCURRENT)
        .try_collect::<Vec<_>>()
        .await?;

        Ok(Out { files, checksums })
    }
}
