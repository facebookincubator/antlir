/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;

use bon::Builder;
use cad_stack::Checksum;
use cad_stack::json_object;
use serde::Deserialize;
use serde::Serialize;
use serde_with::hex::Hex;
use serde_with::serde_as;

use crate::file_content::FileContent;

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Data {
    RegularFile(Checksum<FileContent>),
    Symlink(PathBuf),
    Device(u64),
    Directory,
}

#[serde_as]
#[derive(Debug, PartialEq, Eq, Deserialize, Serialize, Builder)]
#[builder(builder_type(vis = "pub(crate)"))]
pub struct Inode {
    uid: u32,
    gid: u32,
    mode: u32,
    /// Modification time (seconds), recorded only where it is deterministic
    /// (see [crate::add_root_dir_with_mtimes]); 0 everywhere else, which is
    /// also how objects from before mtimes were recorded read. 0 is not
    /// serialized, so those objects keep their checksums. Extraction sets it
    /// as the atime and mtime.
    #[builder(default)]
    #[serde(default, skip_serializing_if = "is_zero")]
    mtime: i64,
    #[builder(default)]
    #[serde_as(as = "Vec<(_, Hex)>")]
    xattrs: BTreeMap<OsString, Vec<u8>>,
    data: Data,
}

impl Inode {
    pub fn uid(&self) -> u32 {
        self.uid
    }

    pub fn gid(&self) -> u32 {
        self.gid
    }

    pub fn mode(&self) -> u32 {
        self.mode
    }

    pub fn mtime(&self) -> i64 {
        self.mtime
    }

    pub fn xattrs(&self) -> &BTreeMap<OsString, Vec<u8>> {
        &self.xattrs
    }

    pub fn content(&self) -> Option<&Checksum<FileContent>> {
        match &self.data {
            Data::RegularFile(checksum) => Some(checksum),
            _ => None,
        }
    }

    pub fn link_target(&self) -> Option<&Path> {
        match &self.data {
            Data::Symlink(target) => Some(target),
            _ => None,
        }
    }

    pub fn rdev(&self) -> Option<u64> {
        match &self.data {
            Data::Device(rdev) => Some(*rdev),
            _ => None,
        }
    }
}

fn is_zero(mtime: &i64) -> bool {
    *mtime == 0
}

json_object!(Inode);
