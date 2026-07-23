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

    // Use cap-std Dir for safe traversal, preventing symlink escape.
    // We do NOT follow symlinks – follow_links(false) avoids loop DoS and
    // outside-tree reads via symlinked dirs.
    let tree_owned = tree.to_owned();
    let file_entries = tokio::task::spawn_blocking(move || -> Result<Vec<(PathBuf, String)>> {
        use std::collections::VecDeque;

        use cap_std::fs::Dir;

        let dir = Dir::open_ambient_dir(&tree_owned, cap_std::ambient_authority())
            .with_context(|| format!("failed to open metadata tree {}", tree_owned.display()))?;

        // BFS walk without following symlinks for containment.
        // We use cap-std to ensure we never escape tree root.
        let mut entries = Vec::new();
        let mut queue: VecDeque<PathBuf> = VecDeque::new();
        queue.push_back(PathBuf::new());

        while let Some(rel_dir) = queue.pop_front() {
            let subdir = if rel_dir.as_os_str().is_empty() {
                // root dir handle already
                // Clone dir for iteration to avoid borrow issues, we keep re-opening via open_dir
                dir.open_dir(".").context("failed to open root dir")?
            } else {
                match dir.open_dir(&rel_dir) {
                    Ok(d) => d,
                    Err(e) => {
                        // Might be symlink or non-dir file – skip
                        debug!("skipping non-dir {:?}: {}", rel_dir, e);
                        continue;
                    }
                }
            };

            for entry_res in subdir.entries()? {
                let entry = entry_res?;
                let file_name = entry.file_name();
                let file_name_str = file_name
                    .to_str()
                    .ok_or_else(|| anyhow::anyhow!("non-utf8 entry {:?}", file_name))?;

                let entry_path = if rel_dir.as_os_str().is_empty() {
                    PathBuf::from(file_name_str)
                } else {
                    rel_dir.join(file_name_str)
                };

                let meta = entry.metadata()?;
                if meta.is_symlink() {
                    // For containment we do NOT follow symlinks to avoid outside-tree access.
                    // This mirrors WalkDir follow_links(false) behavior.
                    debug!("skipping symlink {:?}", entry_path);
                    continue;
                }
                if meta.is_dir() {
                    queue.push_back(entry_path);
                } else if meta.is_file() {
                    // key is rel path string with forward slashes
                    let key = entry_path
                        .to_str()
                        .ok_or_else(|| anyhow::anyhow!("non-utf8 path {:?}", entry_path))?
                        .to_owned();
                    // Full filesystem path for storage
                    let full_path = tree_owned.join(&entry_path);
                    entries.push((full_path, key));
                }
            }
        }

        Ok(entries)
    })
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
