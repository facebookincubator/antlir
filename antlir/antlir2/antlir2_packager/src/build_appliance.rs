/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use antlir2_cas_dir::CasDir;
use serde::de::Deserializer;
use serde::de::Error as _;
use serde::Deserialize;

#[derive(Debug, Clone)]
pub(crate) struct BuildAppliance {
    cas_dir: CasDir,
}

impl BuildAppliance {
    pub(crate) fn unreliable_metadata_contents_path(&self) -> &Path {
        self.cas_dir.unreliable_metadata_contents_path()
    }
}

impl<'de> Deserialize<'de> for BuildAppliance {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = PathBuf::deserialize(deserializer)?;
        CasDir::open(path)
            .map_err(D::Error::custom)
            .map(|cas_dir| Self { cas_dir })
    }
}
