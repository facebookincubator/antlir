/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::BufReader;
use std::io::Read;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use bon::bon;
use serde::Deserialize;
use serde::Serialize;
use sha1::Sha1;
use sha2::Digest;
use sha2::Sha256;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) struct Checksums {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sha1: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) sha256: Option<String>,
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

impl Checksums {
    /// Compute SHA1 and SHA256 checksums from a file on disk.
    pub(crate) fn from_reader<R: Read>(reader: R) -> Result<Self> {
        let mut input = BufReader::new(reader);
        let mut sha1_hasher = Sha1::new();
        let mut sha256_hasher = Sha256::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = input.read(&mut buf).context("failed to read input")?;
            if n == 0 {
                break;
            }
            sha1_hasher.update(&buf[..n]);
            sha256_hasher.update(&buf[..n]);
        }
        Ok(Self {
            sha1: Some(hex::encode(sha1_hasher.finalize())),
            sha256: Some(hex::encode(sha256_hasher.finalize())),
        })
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_from_reader() {
        let input = "Hello world\n".as_bytes();
        let checksums = Checksums::from_reader(input).expect("failed to compute checksums");
        assert_eq!(
            checksums,
            Checksums {
                sha1: Some("33ab5639bfd8e7b95eb1d8d0b87781d4ffea4d5d".to_string()),
                sha256: Some(
                    "1894a19c85ba153acbf743ac4e43fc004c891604b26f8c69e1e83ea2afc7c48f".to_string()
                ),
            }
        );
    }
}
