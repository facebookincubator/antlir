/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use anyhow::Result;
use serde::Deserialize;

use crate::PackageFormat;
mod btrfs_send;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sendstream {
    volume_name: String,
    #[serde(default)]
    incremental_parent: Option<PathBuf>,
    subvol_symlink: PathBuf,
}

impl PackageFormat for Sendstream {
    fn build(&self, out: &Path, layer: &Path) -> Result<()> {
        btrfs_send::build(self, out, layer)
    }
}
