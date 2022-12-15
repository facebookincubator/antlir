/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

/// A buck-built layer target. Currently identified only with the target label,
/// but the location info will be added in a stacked diff.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Layer<'a>(#[serde(borrow)] Label<'a>);

impl<'a> Layer<'a> {
    pub fn label(&self) -> &Label {
        &self.0
    }
}

/// A path on the host, populated by Buck
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct BuckOutSource(PathBuf);

impl BuckOutSource {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// A path inside an image layer
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct PathInLayer(PathBuf);

impl PathInLayer {
    pub fn path(&self) -> &Path {
        &self.0
    }
}
