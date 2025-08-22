/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use super::Fact;
use super::Key;
use crate::fact_impl;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Subvolume {
    path: PathBuf,
}

#[fact_impl("antlir2_facts::fact::subvolume::Subvolume")]
impl Fact for Subvolume {
    fn key(&self) -> Key {
        self.path.as_path().into()
    }
}

impl Subvolume {
    pub fn key(path: &Path) -> Key {
        path.into()
    }

    pub fn new<P>(path: P) -> Self
    where
        P: Into<PathBuf>,
    {
        Self { path: path.into() }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}
