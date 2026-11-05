/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::io::Write as _;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use futures::stream;
use futures::stream::StreamExt;
use futures::stream::TryStreamExt;
use json_arg::Json;
use tracing::debug;

use super::Out;
use super::progress;
use super::storage::Storage;
use super::storage::StorageConfig;

const METADATA_CONCURRENCY: usize = 50;

#[derive(Parser, Debug)]
pub(crate) struct Metadata {
    #[clap(long)]
    out: PathBuf,
    #[clap(long)]
    storage: Json<StorageConfig>,
    #[clap(long)]
    tree: PathBuf,
}

impl Metadata {
    #[tracing::instrument(skip(self, fb), ret, err)]
    pub(crate) async fn run(self, fb: fbinit::FacebookInit) -> Result<()> {
        let storage = self.storage.into_inner().build(fb)?;
        let storage: std::sync::Arc<dyn Storage> = storage.into();
        let out = snapshot_metadata(storage, &self.tree).await?;
        let mut outfile = std::io::BufWriter::new(stdio_path::create(&self.out)?);
        serde_json::to_writer(&mut outfile, &out)?;
        // BufWriter swallows flush errors on drop; flush explicitly so a
        // disk-full / EIO surfaces instead of silently leaving a partial JSON.
        outfile.flush().context("failed to flush metadata output")?;
        Ok(())
    }
}

pub(crate) async fn snapshot_metadata(
    storage: std::sync::Arc<dyn Storage>,
    tree: &Path,
) -> Result<Out> {
    debug!("walking {tree:?}");

    let tree_owned = tree.to_owned();
    let file_entries =
        tokio::task::spawn_blocking(move || snapshot_common::directory_files(&tree_owned))
            .await
            .context("spawn_blocking failed")??;

    let pb = progress::bar(file_entries.len(), "Uploading metadata");

    // Upload with bounded concurrency instead of sequential
    let storage_clone = storage.clone();
    let results: Vec<(String, String, snapshot_common::Checksums)> =
        stream::iter(file_entries.into_iter().map(|(path, key)| {
            let storage = storage_clone.clone();
            let pb = pb.clone();
            async move {
                let result = storage
                    .store(&path, &key)
                    .await
                    .with_context(|| format!("failed to store metadata file {}", path.display()))?;
                pb.inc(1);
                Ok::<_, anyhow::Error>((key, result.url.to_string(), result.checksums))
            }
        }))
        .buffer_unordered(METADATA_CONCURRENCY)
        .try_collect()
        .await?;
    pb.finish_with_message("Uploaded metadata");

    let mut files = BTreeMap::new();
    let mut checksums = BTreeMap::new();
    for (key, url, cs) in results {
        checksums.insert(url.clone(), cs);
        files.insert(key, url);
    }

    Ok(Out { files, checksums })
}
