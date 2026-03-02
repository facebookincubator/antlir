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
use url::Url;

use crate::checksums::Checksums;

#[cfg(facebook)]
mod facebook;
#[cfg(facebook)]
use facebook::ManifoldStorage;
#[cfg(facebook)]
use facebook::ManifoldStorageConfig;

/// Trait for persistent storage backends that can store files and return URLs.
#[async_trait]
pub(crate) trait Storage: Send + Sync {
    /// Store a file and return a (URL, Checksums) pair.
    ///
    /// `key` is the logical path for this file within the storage namespace
    /// (e.g. a relative path within a metadata tree).
    async fn store(&self, file: &Path, key: &str) -> Result<UrlWithChecksums>;
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
