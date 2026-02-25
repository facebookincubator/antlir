/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use anyhow::bail;
use bon::bon;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) struct Checksums {
    #[serde(skip_serializing_if = "Option::is_none")]
    sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    sha256: Option<String>,
}

#[bon]
impl Checksums {
    #[builder]
    pub(crate) fn new(sha1: Option<String>, sha256: Option<String>) -> Result<Self> {
        if sha1.is_none() && sha256.is_none() {
            bail!("At least one checksum must be provided");
        }
        Ok(Self { sha1, sha256 })
    }
}

#[cfg(test)]
impl Checksums {
    pub(crate) fn new_sha256(sha256: String) -> Self {
        Self {
            sha1: None,
            sha256: Some(sha256),
        }
    }
}
