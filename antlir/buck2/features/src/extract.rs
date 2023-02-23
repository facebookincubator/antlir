/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::types::BuckOutSource;
use crate::types::LayerInfo;
use crate::types::PathInLayer;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(
    rename_all = "snake_case",
    tag = "source",
    bound(deserialize = "'de: 'a")
)]
pub enum Extract<'a> {
    Buck(ExtractBuckBinary<'a>),
    Layer(ExtractLayerBinaries<'a>),
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct ExtractBuckBinary<'a> {
    pub src: BuckOutSource<'a>,
    pub dst: PathInLayer<'a>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct ExtractLayerBinaries<'a> {
    pub layer: LayerInfo<'a>,
    pub binaries: Vec<PathInLayer<'a>>,
}
