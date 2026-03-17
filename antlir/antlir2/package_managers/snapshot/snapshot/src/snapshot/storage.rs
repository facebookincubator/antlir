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
    async fn store_flat(&self, file: &Path) -> Result<UrlWithChecksums>;

    /// Check the status of a blob in storage: whether it is missing, expiring
    /// soon, or fresh. This is a single round-trip operation.
    async fn blob_status(&self, checksums: &Checksums) -> Result<BlobStatus>;

    /// Extend the TTL of the object with the given checksums.
    async fn extend_ttl(&self, checksums: &Checksums) -> Result<()>;

    /// Create a tree symlink at `key` pointing to an existing flat blob
    /// identified by `checksums`. Used to (re)build the tree namespace from
    /// content-addressed flat objects that are already known to exist.
    async fn symlink_flat_to_tree(&self, checksums: &Checksums, key: &str) -> Result<()>;
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
#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum StorageConfig {
    #[cfg(facebook)]
    Manifold(ManifoldStorageConfig),
}

impl StorageConfig {
    pub(crate) fn build(self, fb: fbinit::FacebookInit) -> Result<Box<dyn Storage>> {
        match self {
            #[cfg(facebook)]
            StorageConfig::Manifold(c) => Ok(Box::new(ManifoldStorage::new(fb, c)?)),
        }
    }
}
