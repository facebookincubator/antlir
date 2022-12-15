/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::types::Layer;
use crate::types::PathInLayer;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Mount<'a> {
    Host(HostMount),
    #[serde(borrow)]
    Layer(LayerMount<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct HostMount {
    pub mountpoint: PathInLayer,
    pub is_directory: bool,
    pub src: PathBuf,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct LayerMount<'a> {
    pub mountpoint: PathInLayer,
    #[serde(borrow)]
    pub src: Layer<'a>,
}
