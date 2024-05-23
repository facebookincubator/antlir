/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Traits for moving between `antlir2_facts` and `antlir2_depgraph_if`
//! structures.

use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::FsEntry;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_facts::fact::dir_entry::DirEntry;
use antlir2_facts::fact::Fact as _;

pub(crate) trait ItemKeyExt {
    fn fact_kind(&self) -> &'static str;
    fn to_fact_key(&self) -> antlir2_facts::Key;
}

impl ItemKeyExt for ItemKey {
    fn fact_kind(&self) -> &'static str {
        match self {
            Self::Path(_) => antlir2_facts::fact::dir_entry::DirEntry::kind(),
            Self::User(_) => antlir2_facts::fact::user::User::kind(),
            Self::Group(_) => antlir2_facts::fact::user::Group::kind(),
        }
    }

    fn to_fact_key(&self) -> antlir2_facts::Key {
        match self {
            Self::Path(p) => antlir2_facts::fact::dir_entry::DirEntry::key(p),
            Self::User(u) => antlir2_facts::fact::user::User::key(u),
            Self::Group(g) => antlir2_facts::fact::user::Group::key(g),
        }
    }
}

pub(crate) trait FactExt {
    type Item;
    fn to_item(&self) -> Self::Item;
}

impl FactExt for DirEntry {
    type Item = PathItem;

    fn to_item(&self) -> PathItem {
        match self {
            Self::Directory(d) => PathItem::Entry(FsEntry {
                path: d.path().to_owned(),
                file_type: FileType::Directory,
                mode: d.mode(),
            }),
            Self::Symlink(s) => PathItem::Symlink {
                link: s.path().to_owned(),
                target: s.target().to_owned(),
            },
            // This also handles special files (fifo, dev, etc)
            Self::RegularFile(f) => PathItem::Entry(FsEntry {
                path: f.path().to_owned(),
                file_type: FileType::from_mode(f.mode()).unwrap_or(FileType::File),
                mode: f.mode(),
            }),
        }
    }
}
