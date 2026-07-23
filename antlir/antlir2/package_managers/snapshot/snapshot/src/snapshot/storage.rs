/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use anyhow::Result;
use async_trait::async_trait;
use serde::Deserialize;
use serde::Serialize;
use snapshot_common::Checksums;
use url::Url;

#[cfg(facebook)]
mod facebook;
#[cfg(facebook)]
use facebook::ManifoldStorage;
#[cfg(facebook)]
use facebook::ManifoldStorageConfig;

/// The status of a blob in storage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BlobStatus {
    /// The blob does not exist in storage.
    Missing,
    /// The blob exists but its TTL is below the renewal threshold and should be
    /// extended.
    ExpiringSoon,
    /// The blob exists with sufficient TTL remaining.
    Fresh,
}

/// Trait for persistent storage backends that can store files and return URLs.
#[async_trait]
pub(crate) trait Storage: Send + Sync {
    /// Store a file and return a (URL, Checksums) pair.
    /// The file is written to the flat (content-addressed) namespace and
    /// a symlink is created in the tree namespace under `key`.
    async fn store(&self, file: &Path, key: &str) -> Result<UrlWithChecksums>;

    /// Store a file in the flat (content-addressed) namespace only.
    /// No tree symlink is created. Returns a (URL, Checksums) pair where
    /// the URL points to the flat object.
    async fn store_flat(&self, file: &Path) -> Result<UrlWithChecksums> {
        let checksums = Checksums::from_file_async(file.to_owned()).await?;
        self.store_flat_with_checksums(file, &checksums).await
    }

    /// Like `store_flat`, but reuses checksums the caller already computed for
    /// `file`, avoiding a redundant re-hash of the (potentially large) file.
    async fn store_flat_with_checksums(
        &self,
        file: &Path,
        checksums: &Checksums,
    ) -> Result<UrlWithChecksums>;

    /// Check the status of a blob in storage: whether it is missing, expiring
    /// soon, or fresh. This is a single round-trip operation.
    async fn blob_status(&self, checksums: &Checksums) -> Result<BlobStatus>;

    /// Extend the TTL of the object with the given checksums.
    async fn extend_ttl(&self, checksums: &Checksums) -> Result<()>;

    /// Create a tree symlink at `key` pointing to an existing flat blob
    /// identified by `checksums`. Used to (re)build the tree namespace from
    /// content-addressed flat objects that are already known to exist.
    async fn symlink_flat_to_tree(&self, checksums: &Checksums, key: &str) -> Result<()>;

    /// Get the checksums for a file already stored in the tree namespace,
    /// by reading its Manifold metadata. Useful for building a complete
    /// index that contains both sha1 and sha256 even when the original
    /// packages.json only had sha256.
    async fn get_file_checksums(&self, key: &str) -> Result<Checksums>;

    /// Ensure parent directories for the given tree keys exist. This is a
    /// batched, deduplicated version of `ensure_tree_dir` used to pre-warm
    /// the directory cache before bulk symlink creation. The default
    /// implementation is a no-op for backends that don't need explicit dir
    /// creation.
    async fn ensure_tree_dirs(&self, _keys: &[String]) -> Result<()> {
        Ok(())
    }

    /// Check blob status and return full checksums from the flat object's
    /// properties in the same RPC if available. This avoids a second lookup
    /// per file when building the index.
    async fn flat_status_and_full_checksums(
        &self,
        checksums: &Checksums,
    ) -> Result<(BlobStatus, Option<Checksums>)> {
        // Default impl does a single status check and returns no full checksums;
        // callers fall back to the original checksums.
        let status = self.blob_status(checksums).await?;
        Ok((status, None))
    }
}

#[derive(Clone, Deserialize, Serialize)]
pub(crate) struct UrlWithChecksums {
    pub(crate) url: Url,
    pub(crate) checksums: Checksums,
}

impl std::fmt::Debug for UrlWithChecksums {
    #[deny(unused_variables)]
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        let Self { url, checksums } = self;
        f.debug_struct("UrlWithChecksums")
            .field("url", &url.to_string())
            .field("checksums", &checksums)
            .finish()
    }
}

/// Configuration for a storage backend.
/// In OSS builds this enum is uninhabitable (no variants) – build methods return an error instead
/// of non-exhaustive match compile failure.
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum StorageConfig {
    #[cfg(facebook)]
    Manifold(ManifoldStorageConfig),
    #[cfg(not(facebook))]
    #[serde(other)]
    Unsupported,
}

impl StorageConfig {
    pub(crate) fn build(&self, fb: fbinit::FacebookInit) -> Result<Box<dyn Storage>> {
        match self {
            #[cfg(facebook)]
            StorageConfig::Manifold(c) => Ok(Box::new(ManifoldStorage::new(fb, c.clone())?)),
            #[cfg(not(facebook))]
            StorageConfig::Unsupported => {
                let _ = fb;
                anyhow::bail!("Manifold storage is only available in facebook builds")
            }
        }
    }

    pub(crate) fn build_with_tree_prefix(
        &self,
        fb: fbinit::FacebookInit,
        tree_prefix: &str,
    ) -> Result<Box<dyn Storage>> {
        match self {
            #[cfg(facebook)]
            StorageConfig::Manifold(c) => Ok(Box::new(ManifoldStorage::new(
                fb,
                c.with_tree_prefix(tree_prefix),
            )?)),
            #[cfg(not(facebook))]
            StorageConfig::Unsupported => {
                let _ = fb;
                let _ = tree_prefix;
                anyhow::bail!("Manifold storage is only available in facebook builds")
            }
        }
    }

    pub(crate) fn tree_base_url_with_prefix(&self, tree_prefix: &str) -> String {
        match self {
            #[cfg(facebook)]
            StorageConfig::Manifold(c) => c.with_tree_prefix(tree_prefix).tree_base_url(),
            #[cfg(not(facebook))]
            StorageConfig::Unsupported => {
                let _ = tree_prefix;
                String::new()
            }
        }
    }

    /// Render this config as the user-facing dict that would appear in a
    /// rule's `snapshot_storage` attr (i.e. without `tree_prefix`, which is
    /// derived per-target). api_key is intentionally included – it is a non-secret
    /// public identifier separate from access controls per user guidance.
    pub(crate) fn as_user_dict(&self) -> Vec<(&'static str, String)> {
        match self {
            #[cfg(facebook)]
            StorageConfig::Manifold(c) => vec![
                ("type", "manifold".to_owned()),
                ("bucket", c.bucket().to_owned()),
                ("api_key", c.api_key().to_owned()),
            ],
            #[cfg(not(facebook))]
            StorageConfig::Unsupported => vec![],
        }
    }
}
