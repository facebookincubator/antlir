/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

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
    pub subvol_symlink: PathBuf,
    pub facts_db: PathBuf,
}

pub type UserName = String;
pub type GroupName = String;
