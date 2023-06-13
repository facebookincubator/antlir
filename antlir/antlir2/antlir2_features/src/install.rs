/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::os::unix::ffi::OsStrExt;

use serde::de::Error;
use serde::Deserialize;
use serde::Serialize;

use crate::stat::Mode;
use crate::types::BuckOutSource;
use crate::types::PathInLayer;
use crate::usergroup::GroupName;
use crate::usergroup::UserName;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Install<'a> {
    pub dst: PathInLayer<'a>,
    pub group: GroupName<'a>,
    pub mode: Mode,
    pub src: BuckOutSource<'a>,
    pub user: UserName<'a>,
    pub binary_info: Option<BinaryInfo<'a>>,
}

impl<'a> Install<'a> {
    pub fn is_dir(&self) -> bool {
        self.dst.as_os_str().as_bytes().last().copied() == Some(b'/')
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct SplitBinaryMetadata<'a> {
    pub elf: bool,
    #[serde(default)]
    pub buildid: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum BinaryInfo<'a> {
    Dev,
    Installed(InstalledBinary<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct InstalledBinary<'a> {
    pub debuginfo: BuckOutSource<'a>,
    pub metadata: SplitBinaryMetadata<'a>,
}

/// Buck2's `record` will always include `null` values, but serde's native enum
/// deserialization will fail if there are multiple keys, even if the others are
/// null.
/// TODO(vmagro): make this general in the future (either codegen from `record`s
/// or as a proc-macro)
impl<'a, 'de: 'a> Deserialize<'de> for BinaryInfo<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "'de: 'a"))]
        struct Deser<'a> {
            dev: Option<bool>,
            installed: Option<InstalledBinary<'a>>,
        }

        Deser::deserialize(deserializer).and_then(|s| match (s.dev, s.installed) {
            (Some(true), None) => Ok(Self::Dev),
            (None, Some(installed)) => Ok(Self::Installed(installed)),
            (_, _) => Err(D::Error::custom(
                "exactly one of {dev=True, installed} must be set",
            )),
        })
    }
}

impl<'a> Serialize for BinaryInfo<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        #[derive(Serialize)]
        struct Ser<'a, 'b> {
            dev: Option<bool>,
            installed: Option<&'b InstalledBinary<'a>>,
        }
        Ser {
            dev: match self {
                Self::Dev => Some(true),
                _ => None,
            },
            installed: match self {
                Self::Installed(installed) => Some(installed),
                _ => None,
            },
        }
        .serialize(serializer)
    }
}

impl<'a, 'de: 'a> Deserialize<'de> for InstalledBinary<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "'de: 'a"))]
        struct Deser<'a> {
            debuginfo: BuckOutSource<'a>,
            metadata: Metadata<'a>,
        }

        #[derive(Deserialize)]
        #[serde(untagged, bound(deserialize = "'de: 'a"))]
        enum Metadata<'a> {
            Metadata(SplitBinaryMetadata<'a>),
            Path(BuckOutSource<'a>),
        }

        Deser::deserialize(deserializer).and_then(|s| {
            Ok(Self {
                debuginfo: s.debuginfo,
                metadata: match s.metadata {
                    Metadata::Path(path) => {
                        let metadata = std::fs::read(path).map_err(D::Error::custom)?;
                        SplitBinaryMetadata::deserialize(
                            &mut serde_json::Deserializer::from_reader(std::io::Cursor::new(
                                metadata,
                            )),
                        )
                        .map_err(D::Error::custom)?
                    }
                    Metadata::Metadata(metadata) => metadata,
                },
            })
        })
    }
}
