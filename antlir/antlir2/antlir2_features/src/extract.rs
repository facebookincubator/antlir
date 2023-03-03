/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::de::Error;
use serde::Deserialize;
use serde::Serialize;

use crate::types::BuckOutSource;
use crate::types::LayerInfo;
use crate::types::PathInLayer;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case", bound(deserialize = "'de: 'a"))]
pub enum Extract<'a> {
    Buck(ExtractBuckBinary<'a>),
    Layer(ExtractLayerBinaries<'a>),
}

/// Buck2's `record` will always include `null` values, but serde's native enum
/// deserialization will fail if there are multiple keys, even if the others are
/// null.
/// TODO(vmagro): make this general in the future (either codegen from `record`s
/// or as a proc-macro)
impl<'a, 'de: 'a> Deserialize<'de> for Extract<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "'de: 'a"))]
        struct ExtractStruct<'a> {
            buck: Option<ExtractBuckBinary<'a>>,
            layer: Option<ExtractLayerBinaries<'a>>,
        }

        ExtractStruct::deserialize(deserializer).and_then(|s| match (s.buck, s.layer) {
            (Some(v), None) => Ok(Self::Buck(v)),
            (None, Some(v)) => Ok(Self::Layer(v)),
            (_, _) => Err(D::Error::custom("exactly one of {buck, layer} must be set")),
        })
    }
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
