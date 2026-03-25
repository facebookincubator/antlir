/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use anyhow::Context;
use anyhow::Result;
use cad_stack::ObjectStore;
use cap_std::fs::Dir;
use serde::Deserialize;

use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CadStack {}

impl PackageFormat for CadStack {
    fn build(&self, out: &Path, layer: &Path) -> Result<()> {
        std::fs::create_dir_all(out).context("while creating output directory")?;
        let store =
            ObjectStore::new_from_empty(out.join("store")).context("while creating store")?;

        // This collapses the entire layer inheritance chain into a single
        // object store, but that is semantically equivalent to preserving the
        // whole history and actually saves space

        let layer = Dir::open_ambient_dir(layer, cap_std::ambient_authority())
            .context("while opening layer root dir")?;

        cad_stack_fs::add_root_dir(&store, layer).context("while adding root dir to store")?;

        Ok(())
    }
}
