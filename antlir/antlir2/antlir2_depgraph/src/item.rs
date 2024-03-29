/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::hash::Hash;
use std::os::unix::fs::FileTypeExt;
use std::path::PathBuf;

use nix::sys::stat::SFlag;
use serde::Deserialize;
use serde::Serialize;

/// An item that may or may not be provided by a feature in this layer or any of
/// its parents. Used for dependency ordering and conflict checking.
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize
)]
#[serde(rename_all = "snake_case")]
pub enum Item {
    Path(Path),
    User(User),
    Group(Group),
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize
)]
#[serde(rename_all = "snake_case")]
pub enum ItemKey {
    Path(PathBuf),
    User(String),
    Group(String),
}

impl Item {
    pub fn key(&self) -> ItemKey {
        match self {
            Self::Path(p) => match p {
                Path::Entry(e) => ItemKey::Path(e.path.clone()),
                Path::Symlink { link, .. } => ItemKey::Path(link.clone()),
                Path::Mount(mnt) => ItemKey::Path(mnt.path.clone()),
            },
            Self::User(u) => ItemKey::User(u.name.clone()),
            Self::Group(g) => ItemKey::Group(g.name.clone()),
        }
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize
)]
#[serde(rename_all = "snake_case")]
pub enum Path {
    Entry(FsEntry),
    Symlink { link: PathBuf, target: PathBuf },
    Mount(Mount),
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize
)]
pub struct FsEntry {
    pub path: PathBuf,
    pub file_type: FileType,
    pub mode: u32,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize
)]
pub struct Mount {
    pub path: PathBuf,
    pub file_type: FileType,
    pub mode: u32,
    /// Human readable description of where this mount comes from
    pub source_description: String,
}

#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    Hash,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
#[serde(rename_all = "snake_case")]
pub enum FileType {
    File,
    Symlink,
    Directory,
    BlockDevice,
    CharDevice,
    Fifo,
    Socket,
}

impl From<std::fs::FileType> for FileType {
    fn from(f: std::fs::FileType) -> Self {
        if f.is_dir() {
            return Self::Directory;
        }
        if f.is_symlink() {
            return Self::Symlink;
        }
        if f.is_socket() {
            return Self::Socket;
        }
        if f.is_fifo() {
            return Self::Fifo;
        }
        if f.is_char_device() {
            return Self::CharDevice;
        }
        if f.is_block_device() {
            return Self::BlockDevice;
        }
        if f.is_file() {
            return Self::File;
        }
        unreachable!("{f:?}")
    }
}

impl FileType {
    pub fn from_mode(mode: u32) -> Option<Self> {
        let sflag = SFlag::from_bits_truncate(mode);
        if sflag.contains(SFlag::S_IFDIR) {
            Some(Self::Directory)
        } else if sflag.contains(SFlag::S_IFLNK) {
            Some(Self::Symlink)
        } else if sflag.contains(SFlag::S_IFSOCK) {
            Some(Self::Socket)
        } else if sflag.contains(SFlag::S_IFIFO) {
            Some(Self::Fifo)
        } else if sflag.contains(SFlag::S_IFCHR) {
            Some(Self::CharDevice)
        } else if sflag.contains(SFlag::S_IFBLK) {
            Some(Self::BlockDevice)
        } else if sflag.contains(SFlag::S_IFREG) {
            Some(Self::File)
        } else {
            None
        }
    }
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize
)]
pub struct User {
    pub name: String,
    // there is more information available about users, but it's not necessary
    // for the depgraph
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Deserialize,
    Serialize
)]
pub struct Group {
    pub name: String,
}
