/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! This module contains the data structures that are used by buck2 rules
//! referencing [OverlayFs] [Layer]s.

use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct OverlayFs {
    pub(crate) top: Layer,
    pub(crate) layers: Vec<Layer>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub(crate) struct Layer {
    pub(crate) data_dir: PathBuf,
    /// Path to a [crate::manifest::Manifest] file
    pub(crate) manifest: PathBuf,
}
