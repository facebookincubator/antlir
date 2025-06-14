/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::Metadata;
use std::os::unix::fs::MetadataExt;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use super::Fact;
use super::Key;
use crate::fact_impl;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub enum DirEntry {
    Directory(Directory),
    Symlink(Symlink),
    RegularFile(RegularFile),
}

#[fact_impl("antlir2_facts::fact::dir_entry::DirEntry")]
impl Fact for DirEntry {
    fn key(&self) -> Key {
        match self {
            Self::Directory(d) => d.common.path.as_path().into(),
            Self::Symlink(s) => s.common.path.as_path().into(),
            Self::RegularFile(f) => f.common.path.as_path().into(),
        }
    }
}

macro_rules! proxy_file_common {
    () => {
        #[inline]
        pub fn path(&self) -> &Path {
            self.common().path()
        }

        #[inline]
        pub fn uid(&self) -> u32 {
            self.common().uid()
        }

        #[inline]
        pub fn gid(&self) -> u32 {
            self.common().gid()
        }

        #[inline]
        pub fn mode(&self) -> u32 {
            self.common().mode()
        }
    };
}

impl DirEntry {
    pub fn key(path: &Path) -> Key {
        path.into()
    }

    fn common(&self) -> &FileCommon {
        match self {
            Self::Directory(d) => &d.common,
            Self::Symlink(s) => &s.common,
            Self::RegularFile(f) => &f.common,
        }
    }

    proxy_file_common!();
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FileCommon {
    path: PathBuf,
    uid: u32,
    gid: u32,
    mode: u32,
}

impl FileCommon {
    pub fn new(path: PathBuf, uid: u32, gid: u32, mode: u32) -> Self {
        Self {
            path,
            uid,
            gid,
            mode,
        }
    }

    #[cfg(unix)]
    pub fn new_with_metadata(path: PathBuf, metadata: &Metadata) -> Self {
        Self {
            path,
            uid: metadata.uid(),
            gid: metadata.gid(),
            mode: metadata.mode(),
        }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn gid(&self) -> u32 {
        self.gid
    }

    pub fn mode(&self) -> u32 {
        self.mode
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Directory {
    #[serde(flatten)]
    common: FileCommon,
}

impl From<FileCommon> for Directory {
    fn from(value: FileCommon) -> Self {
        Self { common: value }
    }
}

impl Directory {
    fn common(&self) -> &FileCommon {
        &self.common
    }

    proxy_file_common!();
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Symlink {
    #[serde(flatten)]
    common: FileCommon,
    /// Where the target actually points (one level only)
    target: PathBuf,
    /// The actual link contents (may be relative)
    raw_target: PathBuf,
}

impl Symlink {
    pub fn new(common: FileCommon, raw_target: PathBuf) -> Self {
        let mut target = common.path().parent().unwrap_or(Path::new("/")).to_owned();
        for component in raw_target.components() {
            match component {
                Component::Prefix(_) => unreachable!("only linux is supported"),
                Component::RootDir => {
                    target.push("/");
                }
                Component::CurDir => {}
                Component::ParentDir => {
                    target.pop();
                }
                Component::Normal(n) => {
                    target.push(n);
                }
            }
        }
        Self {
            common,
            target,
            raw_target,
        }
    }

    /// Absolute (within the layer) path to the target
    pub fn target(&self) -> &Path {
        &self.target
    }

    /// Actual contents of the symlink (may be relative)
    pub fn raw_target(&self) -> &Path {
        &self.raw_target
    }
}

impl Symlink {
    fn common(&self) -> &FileCommon {
        &self.common
    }

    proxy_file_common!();
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct RegularFile {
    #[serde(flatten)]
    common: FileCommon,
}

impl From<FileCommon> for RegularFile {
    fn from(value: FileCommon) -> Self {
        Self { common: value }
    }
}

impl RegularFile {
    fn common(&self) -> &FileCommon {
        &self.common
    }

    proxy_file_common!();
}
