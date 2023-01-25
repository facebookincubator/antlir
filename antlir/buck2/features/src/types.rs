/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::path::Path;

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

impl<'a> From<Label<'a>> for Layer<'a> {
    fn from(label: Label<'a>) -> Self {
        Self(label)
    }
}

/// A path on the host, populated by Buck
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct BuckOutSource<'a>(Cow<'a, Path>);

impl<'a> BuckOutSource<'a> {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

/// A path inside an image layer
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct PathInLayer<'a>(#[serde(borrow)] Cow<'a, Path>);

impl<'a> PathInLayer<'a> {
    pub fn path(&self) -> &Path {
        &self.0
    }
}

impl<'a, P> From<P> for PathInLayer<'a>
where
    P: Into<Cow<'a, Path>>,
{
    fn from(p: P) -> Self {
        Self(p.into())
    }
}
