/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use antlir2_overlayfs::BuckModel as OverlayfsModel;
use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

/// A path on the host, populated by Buck
pub type BuckOutSource = PathBuf;
/// A path inside an image layer
pub type PathInLayer = PathBuf;

/// Serialized buck2 LayerInfo provider
#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize,
    Hash
)]
pub struct LayerInfo {
    pub label: Label,
    pub facts_db: PathBuf,
    pub contents: LayerContents,
}

pub type UserName = String;
pub type GroupName = String;

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize,
    Hash
)]
#[serde(rename_all = "snake_case")]
pub enum LayerContents {
    SubvolSymlink(PathBuf),
    Overlayfs(OverlayfsModel),
}

impl LayerContents {
    pub fn as_subvol_symlink(&self) -> Option<&Path> {
        match self {
            Self::SubvolSymlink(p) => Some(p),
            _ => None,
        }
    }

    pub fn as_overlayfs(&self) -> Option<&OverlayfsModel> {
        match self {
            Self::Overlayfs(m) => Some(m),
            _ => None,
        }
    }
}
