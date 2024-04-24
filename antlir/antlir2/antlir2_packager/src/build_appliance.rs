/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
pub(crate) struct BuildAppliance(PathBuf);

impl BuildAppliance {
    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}
