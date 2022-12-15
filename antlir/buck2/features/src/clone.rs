/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::types::Layer;
use crate::types::PathInLayer;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Clone<'a> {
    #[serde(borrow)]
    pub src_layer: Layer<'a>,
    pub omit_outer_dir: bool,
    pub pre_existing_dest: bool,
    pub src_path: PathInLayer,
    pub dst: PathInLayer,
}
