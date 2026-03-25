/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::PathBuf;

use bon::Builder;
use cad_stack::Checksum;
use cad_stack::json_object;
use serde::Deserialize;
use serde::Serialize;

use super::meta::Inode;

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize, Builder)]
#[builder(builder_type(vis = "pub(crate)"))]
pub struct Dir {
    meta: Checksum<Inode>,
    #[builder(default)]
    entries: BTreeMap<String, DirEntry>,
}

impl Dir {
    pub(crate) fn add_directory(&mut self, name: String, dir: Checksum<Dir>) {
        self.entries.insert(name, DirEntry::Dir(dir));
    }

    pub(crate) fn add_file(&mut self, name: String, meta: Checksum<Inode>) {
        self.entries.insert(name, DirEntry::File(meta));
    }

    pub(crate) fn add_hardlink(
        &mut self,
        name: String,
        first_target: PathBuf,
        meta: Checksum<Inode>,
    ) {
        self.entries.insert(
            name,
            DirEntry::Hardlink {
                first_target,
                inode: meta,
            },
        );
    }

    /// Get the metadata checksum for this directory.
    pub fn meta(&self) -> &Checksum<Inode> {
        &self.meta
    }

    /// Get an iterator over the entries in this directory.
    ///
    /// Entries are returned in sorted order by name (due to BTreeMap).
    pub fn entries(&self) -> impl Iterator<Item = (&String, &DirEntry)> {
        self.entries.iter()
    }

    /// Get a specific entry by name.
    pub fn get(&self, name: &str) -> Option<&DirEntry> {
        self.entries.get(name)
    }

    /// Returns the number of entries in this directory.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if this directory has no entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum DirEntry {
    Dir(Checksum<Dir>),
    File(Checksum<Inode>),
    /// Ideally a hardlink could just be stored as another Checksum<Inode>, but
    /// we need to be able to distinguish between files that were actually
    /// hardlinks and files that just happened to be identical in every way, but
    /// should not be materialized as hardlinks when extracting
    Hardlink {
        first_target: PathBuf,
        inode: Checksum<Inode>,
    },
}

json_object!(Dir);
