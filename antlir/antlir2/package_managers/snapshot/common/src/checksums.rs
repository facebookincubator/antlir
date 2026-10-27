/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::BufReader;
use std::io::Read;
use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use bon::bon;
use serde::Deserialize;
use serde::Serialize;
use serde::ser::SerializeMap as _;
use serde_with::hex::Hex;
use serde_with::serde_as;
use sha1::Sha1;
use sha2::Digest;
use sha2::Sha256;
use sha2::Sha512;

/// First-class error type for checksum verification using thiserror.
#[derive(Debug, Clone, thiserror::Error, PartialEq, Eq)]
pub enum ChecksumVerificationError {
    #[error("sha256 mismatch: expected {expected} got {actual}")]
    Sha256Mismatch { expected: String, actual: String },

    #[error("sha512 mismatch: expected {expected} got {actual}")]
    Sha512Mismatch { expected: String, actual: String },

    #[error("sha1 mismatch: expected {expected} got {actual}")]
    Sha1Mismatch { expected: String, actual: String },

    #[error(
        "no checksum matched – at least one matching checksum required (expected {expected:?} vs actual {actual:?})"
    )]
    NoMatchingChecksum {
        expected: Box<Checksums>,
        actual: Box<Checksums>,
    },
}

// `serde_as(Hex)` handles the hex-string <-> bytes conversion on deserialize.
// Serialize is hand-written on purpose: it must emit a serde *map* rather than a
// *struct*. The generated BUCK files are produced via `serde_starlark`, which
// renders a struct as a Starlark function call (`Checksums(sha1 = ...)`) but a
// map as a dict (`{"sha1": ...}`). The suite/repo rules expect a dict, so a
// derived `Serialize` (which drives `serialize_struct`) would break BUCK
// generation. Do not replace this with `#[derive(Serialize)]`.
#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize)]
#[serde(rename_all = "lowercase")]
pub struct Checksums {
    #[serde_as(as = "Option<Hex>")]
    #[serde(default)]
    pub sha1: Option<[u8; 20]>,
    #[serde_as(as = "Option<Hex>")]
    #[serde(default)]
    pub sha256: Option<[u8; 32]>,
    #[serde_as(as = "Option<Hex>")]
    #[serde(default)]
    pub sha512: Option<[u8; 64]>,
}

impl Serialize for Checksums {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut map = serializer.serialize_map(None)?;
        if let Some(sha1) = &self.sha1 {
            map.serialize_entry("sha1", &hex::encode(sha1))?;
        }
        if let Some(sha256) = &self.sha256 {
            map.serialize_entry("sha256", &hex::encode(sha256))?;
        }
        if let Some(sha512) = &self.sha512 {
            map.serialize_entry("sha512", &hex::encode(sha512))?;
        }
        map.end()
    }
}

fn decode_hex<const N: usize>(s: &str) -> Result<[u8; N]> {
    let bytes = hex::decode(s).with_context(|| format!("invalid hex '{}'", s))?;
    if bytes.len() != N {
        bail!(
            "must be {} hex chars ({} bytes), got {} chars ({} bytes): '{}'",
            N * 2,
            N,
            s.len(),
            bytes.len(),
            s
        );
    }
    let mut arr = [0u8; N];
    arr.copy_from_slice(&bytes);
    Ok(arr)
}

#[bon]
impl Checksums {
    #[builder]
    pub fn new(
        sha1: Option<String>,
        sha256: Option<String>,
        sha512: Option<String>,
    ) -> Result<Self> {
        if sha1.is_none() && sha256.is_none() && sha512.is_none() {
            bail!("At least one checksum must be provided");
        }
        let sha1_bytes = sha1
            .map(|s| decode_hex::<20>(&s))
            .transpose()
            .context("invalid sha1")?;
        let sha256_bytes = sha256
            .map(|s| decode_hex::<32>(&s))
            .transpose()
            .context("invalid sha256")?;
        let sha512_bytes = sha512
            .map(|s| decode_hex::<64>(&s))
            .transpose()
            .context("invalid sha512")?;
        Ok(Self {
            sha1: sha1_bytes,
            sha256: sha256_bytes,
            sha512: sha512_bytes,
        })
    }
}

impl Checksums {
    /// Sync reader – for sync contexts.
    pub fn from_reader<R: Read>(reader: R) -> Result<Self> {
        let mut input = BufReader::new(reader);
        let mut sha1_hasher = Sha1::new();
        let mut sha256_hasher = Sha256::new();
        let mut sha512_hasher = Sha512::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = input.read(&mut buf).context("failed to read input")?;
            if n == 0 {
                break;
            }
            sha1_hasher.update(&buf[..n]);
            sha256_hasher.update(&buf[..n]);
            sha512_hasher.update(&buf[..n]);
        }
        Ok(Self {
            sha1: Some(sha1_hasher.finalize().into()),
            sha256: Some(sha256_hasher.finalize().into()),
            sha512: Some(sha512_hasher.finalize().into()),
        })
    }

    /// Async variant – encapsulates async I/O so callers don't need spawn_blocking.
    pub async fn from_async_reader<R>(mut reader: R) -> Result<Self>
    where
        R: tokio::io::AsyncRead + Unpin + Send,
    {
        use tokio::io::AsyncReadExt;
        let mut sha1_hasher = Sha1::new();
        let mut sha256_hasher = Sha256::new();
        let mut sha512_hasher = Sha512::new();
        let mut buf = [0u8; 64 * 1024];
        loop {
            let n = reader
                .read(&mut buf)
                .await
                .context("failed to read input")?;
            if n == 0 {
                break;
            }
            sha1_hasher.update(&buf[..n]);
            sha256_hasher.update(&buf[..n]);
            sha512_hasher.update(&buf[..n]);
        }
        Ok(Self {
            sha1: Some(sha1_hasher.finalize().into()),
            sha256: Some(sha256_hasher.finalize().into()),
            sha512: Some(sha512_hasher.finalize().into()),
        })
    }

    /// Async helper – hash file at path without requiring caller to do blocking work.
    pub async fn from_file_async<P>(path: P) -> Result<Self>
    where
        P: AsRef<Path> + Send + 'static,
    {
        let file = tokio::fs::File::open(path.as_ref())
            .await
            .with_context(|| format!("failed to open {}", path.as_ref().display()))?;
        Self::from_async_reader(file).await
    }

    pub fn new_sha256(sha256: [u8; 32]) -> Self {
        Self {
            sha1: None,
            sha256: Some(sha256),
            sha512: None,
        }
    }

    pub fn new_sha512(sha512: [u8; 64]) -> Self {
        Self {
            sha1: None,
            sha256: None,
            sha512: Some(sha512),
        }
    }

    pub fn new_sha1(sha1: [u8; 20]) -> Self {
        Self {
            sha1: Some(sha1),
            sha256: None,
            sha512: None,
        }
    }

    /// Convenience: create from hex string, failing if invalid.
    pub fn new_sha256_hex<S: AsRef<str>>(hex_str: S) -> Result<Self> {
        Ok(Self {
            sha1: None,
            sha256: Some(decode_hex::<32>(hex_str.as_ref())?),
            sha512: None,
        })
    }

    /// Convenience: create from hex string, failing if invalid.
    pub fn new_sha512_hex<S: AsRef<str>>(hex_str: S) -> Result<Self> {
        Ok(Self {
            sha1: None,
            sha256: None,
            sha512: Some(decode_hex::<64>(hex_str.as_ref())?),
        })
    }

    /// Convenience: create from hex string, failing if invalid.
    pub fn new_sha1_hex<S: AsRef<str>>(hex_str: S) -> Result<Self> {
        Ok(Self {
            sha1: Some(decode_hex::<20>(hex_str.as_ref())?),
            sha256: None,
            sha512: None,
        })
    }

    pub fn sha1_hex(&self) -> Option<String> {
        self.sha1.as_ref().map(hex::encode)
    }

    pub fn sha256_hex(&self) -> Option<String> {
        self.sha256.as_ref().map(hex::encode)
    }

    pub fn sha512_hex(&self) -> Option<String> {
        self.sha512.as_ref().map(hex::encode)
    }

    /// Verify that checksums don't disagree and at least one matches.
    pub fn verify_against(
        &self,
        actual: &Self,
    ) -> std::result::Result<(), ChecksumVerificationError> {
        if let (Some(exp), Some(act)) = (&self.sha256, &actual.sha256) {
            if exp != act {
                return Err(ChecksumVerificationError::Sha256Mismatch {
                    expected: hex::encode(exp),
                    actual: hex::encode(act),
                });
            }
        }
        if let (Some(exp), Some(act)) = (&self.sha512, &actual.sha512) {
            if exp != act {
                return Err(ChecksumVerificationError::Sha512Mismatch {
                    expected: hex::encode(exp),
                    actual: hex::encode(act),
                });
            }
        }
        if let (Some(exp), Some(act)) = (&self.sha1, &actual.sha1) {
            if exp != act {
                return Err(ChecksumVerificationError::Sha1Mismatch {
                    expected: hex::encode(exp),
                    actual: hex::encode(act),
                });
            }
        }
        let matched = (self.sha256.is_some() && actual.sha256.is_some())
            || (self.sha512.is_some() && actual.sha512.is_some())
            || (self.sha1.is_some() && actual.sha1.is_some());
        if matched {
            Ok(())
        } else {
            Err(ChecksumVerificationError::NoMatchingChecksum {
                expected: Box::new(self.clone()),
                actual: Box::new(actual.clone()),
            })
        }
    }
}

#[cfg(test)]
mod tests {
    use hex_literal::hex;

    use super::*;

    #[test]
    fn test_from_reader() {
        let input = "Hello world\n".as_bytes();
        let checksums = Checksums::from_reader(input).expect("failed to compute checksums");
        assert_eq!(
            checksums,
            Checksums {
                sha1: Some(hex!("33ab5639bfd8e7b95eb1d8d0b87781d4ffea4d5d")),
                sha256: Some(hex!(
                    "1894a19c85ba153acbf743ac4e43fc004c891604b26f8c69e1e83ea2afc7c48f"
                )),
                sha512: Some(hex!(
                    "81381f1dacd4824a6c503fd07057763099c12b8309d0abcec4000c9060cbbfa67988b2ada669ab4837fcd3d4ea6e2b8db2b9da9197d5112fb369fd006da545de"
                )),
            }
        );
    }

    #[test]
    fn test_serde_roundtrip() {
        let cs = Checksums {
            sha1: Some(hex!("33ab5639bfd8e7b95eb1d8d0b87781d4ffea4d5d")),
            sha256: Some(hex!(
                "1894a19c85ba153acbf743ac4e43fc004c891604b26f8c69e1e83ea2afc7c48f"
            )),
            sha512: Some(hex!(
                "81381f1dacd4824a6c503fd07057763099c12b8309d0abcec4000c9060cbbfa67988b2ada669ab4837fcd3d4ea6e2b8db2b9da9197d5112fb369fd006da545de"
            )),
        };
        let json = serde_json::to_string(&cs).expect("serializing checksums should succeed");
        assert!(json.contains("33ab5639bfd8e7b95eb1d8d0b87781d4ffea4d5d"));
        assert!(json.contains("1894a19c85ba153acbf743ac4e43fc004c891604b26f8c69e1e83ea2afc7c48f"));
        assert!(json.contains("81381f1dacd4824a6c503fd07057763099c12b8309d0abcec4000c9060cbbfa67988b2ada669ab4837fcd3d4ea6e2b8db2b9da9197d5112fb369fd006da545de"));
        let de: Checksums =
            serde_json::from_str(&json).expect("deserializing checksums should succeed");
        assert_eq!(cs, de);
    }

    #[test]
    fn test_serde_skip_none() {
        let cs = Checksums {
            sha1: None,
            sha256: Some([0xAA; 32]),
            sha512: None,
        };
        let json = serde_json::to_string(&cs).expect("serializing checksums should succeed");
        assert!(json.contains("sha256"));
        assert!(
            !json.contains("sha1"),
            "sha1 should be skipped when None: {json}"
        );
        assert!(
            !json.contains("sha512"),
            "sha512 should be skipped when None: {json}"
        );
    }

    #[test]
    fn test_serializes_as_starlark_dict_not_call() {
        // Generated BUCK files render Checksums via serde_starlark. It must
        // serialize as a Starlark dict `{...}`, not a `Checksums(...)` call —
        // the latter references a nonexistent function and breaks the BUCK file.
        let cs = Checksums {
            sha1: Some(hex!("33ab5639bfd8e7b95eb1d8d0b87781d4ffea4d5d")),
            sha256: Some(hex!(
                "1894a19c85ba153acbf743ac4e43fc004c891604b26f8c69e1e83ea2afc7c48f"
            )),
            sha512: None,
        };
        let starlark = serde_starlark::to_string(&cs).expect("serializing to starlark");
        assert!(
            starlark.trim_start().starts_with('{'),
            "checksums must serialize as a dict, got: {starlark}"
        );
        assert!(
            !starlark.contains("Checksums("),
            "checksums must not serialize as a function call, got: {starlark}"
        );
        assert!(
            starlark.contains(
                "\"sha256\": \"1894a19c85ba153acbf743ac4e43fc004c891604b26f8c69e1e83ea2afc7c48f\""
            ),
            "got: {starlark}"
        );
    }

    #[test]
    fn test_builder_validates_hex() {
        // valid
        let cs = Checksums::builder()
            .sha256("1894a19c85ba153acbf743ac4e43fc004c891604b26f8c69e1e83ea2afc7c48f".to_string())
            .build()
            .expect("valid sha256 should succeed");
        assert!(cs.sha256.is_some());

        // invalid hex chars
        let err = Checksums::builder()
            .sha256("zzzz".to_string())
            .build()
            .expect_err("invalid hex should fail");
        assert!(format!("{err:#}").contains("invalid"));

        // wrong length
        let err = Checksums::builder()
            .sha256("abc123".to_string())
            .build()
            .expect_err("wrong length should fail");
        assert!(format!("{err:#}").contains("64"));
    }

    #[test]
    fn test_verify_against() {
        let exp = Checksums {
            sha256: Some([0xAA; 32]),
            sha1: Some([0xCC; 20]),
            sha512: None,
        };
        let act = Checksums {
            sha256: Some([0xAA; 32]),
            sha1: None,
            sha512: None,
        };
        assert!(exp.verify_against(&act).is_ok());

        let exp2 = Checksums {
            sha256: Some([0xAA; 32]),
            sha1: None,
            sha512: None,
        };
        let act2 = Checksums {
            sha256: Some([0xAA; 32]),
            sha1: Some([0xDD; 20]),
            sha512: None,
        };
        assert!(exp2.verify_against(&act2).is_ok());

        let exp3 = Checksums {
            sha256: Some([0xAA; 32]),
            sha1: None,
            sha512: None,
        };
        let act3 = Checksums {
            sha256: None,
            sha1: Some([0xCC; 20]),
            sha512: None,
        };
        assert!(exp3.verify_against(&act3).is_err());

        // sha512-only expected matches sha512-capable actual
        let exp4 = Checksums {
            sha256: None,
            sha1: None,
            sha512: Some([0xBB; 64]),
        };
        let act4 = Checksums {
            sha256: Some([0xAA; 32]),
            sha1: Some([0xCC; 20]),
            sha512: Some([0xBB; 64]),
        };
        assert!(exp4.verify_against(&act4).is_ok());

        // sha512 mismatch is an error
        let act5 = Checksums {
            sha256: None,
            sha1: None,
            sha512: Some([0xCC; 64]),
        };
        assert!(exp4.verify_against(&act5).is_err());

        let empty = Checksums {
            sha256: None,
            sha1: None,
            sha512: None,
        };
        assert!(empty.verify_against(&empty).is_err());
    }
}
