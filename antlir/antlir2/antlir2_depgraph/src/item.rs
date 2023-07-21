/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::hash::Hash;
use std::os::unix::fs::FileTypeExt;

use buck_label::Label;
use derivative::Derivative;
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
pub enum Item<'a> {
    Path(Path<'a>),
    User(User<'a>),
    Group(Group<'a>),
    /// A complete graph from a dependent layer. Note that items from the chain
    /// of parent layers will appear in this graph, and this is for things like
    /// [antlir2_features::Clone] that have dependencies on (potentially) completely
    /// disconnected layers.
    #[serde(borrow)]
    Layer(Layer<'a>),
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
pub enum ItemKey<'a> {
    Path(Cow<'a, std::path::Path>),
    User(Cow<'a, str>),
    Group(Cow<'a, str>),
    #[serde(borrow)]
    Layer(Label<'a>),
}

impl<'a> Item<'a> {
    pub fn key(&self) -> ItemKey<'a> {
        match self {
            Self::Path(p) => match p {
                Path::Entry(e) => ItemKey::Path(e.path.clone()),
                Path::Symlink { link, .. } => ItemKey::Path(link.clone()),
            },
            Self::User(u) => ItemKey::User(u.name.clone()),
            Self::Group(g) => ItemKey::Group(g.name.clone()),
            Self::Layer(l) => ItemKey::Layer(l.label.clone()),
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
pub enum Path<'a> {
    Entry(FsEntry<'a>),
    Symlink {
        link: Cow<'a, std::path::Path>,
        target: Cow<'a, std::path::Path>,
    },
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
pub struct FsEntry<'a> {
    pub path: Cow<'a, std::path::Path>,
    pub file_type: FileType,
    pub mode: u32,
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
pub struct User<'a> {
    pub name: Cow<'a, str>,
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
pub struct Group<'a> {
    pub name: Cow<'a, str>,
}

#[derive(Clone, Derivative, Deserialize, Serialize)]
#[derivative(Debug)]
pub struct Layer<'a> {
    #[serde(borrow)]
    pub(crate) label: Label<'a>,
    #[derivative(Debug = "ignore")]
    pub(crate) graph: crate::Graph<'a>,
}

impl<'a> PartialEq for Layer<'a> {
    fn eq(&self, other: &Self) -> bool {
        self.label == other.label
    }
}

impl<'a> Eq for Layer<'a> {}

impl<'a> Hash for Layer<'a> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.label.hash(state)
    }
}

impl<'a> PartialOrd for Layer<'a> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        self.label.partial_cmp(&other.label)
    }
}

impl<'a> Ord for Layer<'a> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.label.cmp(&other.label)
    }
}
