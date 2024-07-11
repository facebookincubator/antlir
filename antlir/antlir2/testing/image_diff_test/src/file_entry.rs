/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fmt::Debug;
use std::fmt::Display;
use std::os::unix::fs::FileTypeExt;
use std::os::unix::prelude::MetadataExt;
use std::path::Path;
use std::str::FromStr;

use antlir2_facts::fact::user::Group;
use antlir2_facts::fact::user::User;
use antlir2_mode::Mode;
use anyhow::Context;
use anyhow::Result;
use md5::Digest;
use md5::Md5;
use serde::de::Error as _;
use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use serde_with::DisplayFromStr;

#[serde_as]
#[serde_with::skip_serializing_none]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct FileEntry {
    #[serde_as(as = "DisplayFromStr")]
    pub(crate) mode: Mode,
    #[serde_as(as = "DisplayFromStr")]
    pub(crate) file_type: FileType,
    pub(crate) user: NameOrId,
    pub(crate) group: NameOrId,
    #[serde(default)]
    pub(crate) text: Option<String>,

    #[serde(default)]
    pub(crate) content_hash: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) xattrs: BTreeMap<String, XattrData>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(untagged)]
pub enum NameOrId {
    Name(String),
    Id(u32),
}

impl FileEntry {
    pub fn new(
        path: &Path,
        users: &HashMap<u32, User>,
        groups: &HashMap<u32, Group>,
    ) -> Result<Self> {
        let meta = std::fs::symlink_metadata(path).context("while statting file")?;
        let mode = Mode::from(meta.permissions());
        if meta.is_symlink() {
            let target = std::fs::read_link(path).context("while reading symlink target")?;
            // symlinks do not have xattrs or many other properties of a file,
            // so we just put the symlink in as the text content
            return Ok(Self {
                mode,
                file_type: FileType::from(meta.file_type()),
                user: users
                    .get(&meta.uid())
                    .map(|u| NameOrId::Name(u.name().to_owned()))
                    .unwrap_or(NameOrId::Id(meta.uid())),
                group: groups
                    .get(&meta.gid())
                    .map(|g| NameOrId::Name(g.name().to_owned()))
                    .unwrap_or(NameOrId::Id(meta.gid())),
                text: Some(
                    target
                        .to_str()
                        .context("symlink target is not utf8")?
                        .to_owned(),
                ),
                content_hash: None,
                xattrs: Default::default(),
            });
        }
        let (text, content_hash) = if meta.is_file() {
            let contents = std::fs::read(path).context("while reading file")?;
            let mut hasher = Md5::new();
            hasher.update(&contents);
            (
                String::from_utf8(contents).ok(),
                Some(format!("{:x}", hasher.finalize())),
            )
        } else {
            (None, None)
        };
        let xattrs = xattr::list(path)
            .context("while listing xattrs")?
            .map(|name| {
                name.into_string()
                    .expect("all xattrs we care about are utf8")
            })
            // We really don't care about selinux xattrs since they are very
            // dependent on system configuration
            .filter(|name| name != "security.selinux")
            .filter_map(|name| {
                xattr::get(path, &name)
                    .context("while reading xattr")
                    .map(|value| value.map(|value| (name, XattrData(value))))
                    .transpose()
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            mode,
            user: users
                .get(&meta.uid())
                .map(|u| NameOrId::Name(u.name().to_owned()))
                .unwrap_or(NameOrId::Id(meta.uid())),
            group: groups
                .get(&meta.gid())
                .map(|g| NameOrId::Name(g.name().to_owned()))
                .unwrap_or(NameOrId::Id(meta.gid())),
            file_type: FileType::from(meta.file_type()),
            xattrs,
            content_hash: if text.is_none() { content_hash } else { None },
            text,
        })
    }
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub enum FileType {
    BlockDevice,
    CharacterDevice,
    Directory,
    Fifo,
    RegularFile,
    Socket,
    Symlink,
}

impl From<std::fs::FileType> for FileType {
    fn from(f: std::fs::FileType) -> Self {
        if f.is_block_device() {
            // technically a device could be (and always? is) both a block and
            // character device, but we want to report it as a block device here
            Self::BlockDevice
        } else if f.is_char_device() {
            Self::CharacterDevice
        } else if f.is_dir() {
            Self::Directory
        } else if f.is_fifo() {
            Self::Fifo
        } else if f.is_socket() {
            Self::Socket
        } else if f.is_symlink() {
            Self::Symlink
        } else if f.is_file() {
            Self::RegularFile
        } else {
            unreachable!("everything should fall under one of those")
        }
    }
}

impl Display for FileType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::BlockDevice => "block-device",
            Self::CharacterDevice => "character-device",
            Self::Directory => "directory",
            Self::Fifo => "fifo",
            Self::RegularFile => "regular-file",
            Self::Socket => "socket",
            Self::Symlink => "symlink",
        })
    }
}

impl FromStr for FileType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "block-device" => Ok(Self::BlockDevice),
            "character-device" => Ok(Self::CharacterDevice),
            "directory" => Ok(Self::Directory),
            "fifo" => Ok(Self::Fifo),
            "regular-file" => Ok(Self::RegularFile),
            "socket" => Ok(Self::Socket),
            "symlink" => Ok(Self::Symlink),
            _ => Err(format!("unknown filetype: '{s}'")),
        }
    }
}

#[derive(Clone, PartialEq, Eq)]
pub(crate) struct XattrData(pub(crate) Vec<u8>);

impl Serialize for XattrData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match std::str::from_utf8(&self.0) {
            Ok(text) => serializer.serialize_str(text),
            Err(_) => self.0.serialize(serializer),
        }
    }
}

impl Debug for XattrData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match std::str::from_utf8(&self.0) {
            Ok(text) => f.debug_tuple("XattrData").field(&text).finish(),
            Err(_) => f.debug_tuple("XattrData").field(&self.0).finish(),
        }
    }
}

impl<'de> Deserialize<'de> for XattrData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        if let Some(hex_value) = s.strip_prefix("0x") {
            let bytes = hex::decode(hex_value).map_err(D::Error::custom)?;
            Ok(Self(bytes))
        } else if let Some(b64) = s.strip_prefix("0s") {
            let bytes = base64::decode(b64).map_err(D::Error::custom)?;
            Ok(Self(bytes))
        } else {
            Ok(Self(s.into_bytes()))
        }
    }
}
