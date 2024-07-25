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
mod userspace;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Sendstream {
    volume_name: String,
    #[serde(default)]
    incremental_parent: Option<PathBuf>,
    subvol_symlink: Option<PathBuf>,
    userspace: bool,
}

impl PackageFormat for Sendstream {
    fn build(&self, out: &Path, layer: &Path) -> Result<()> {
        match self.userspace {
            false => btrfs_send::build(self, out, layer),
            true => userspace::build(self, out, layer),
        }
    }
}
