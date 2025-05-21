/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::hash::Hash;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;

use crate::Error;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) struct PluginJson {
    pub(crate) plugin: PathBuf,
    libs: PathBuf,
}

pub struct Plugin {
    path: PathBuf,
    lib: &'static libloading::Library,
}

impl Plugin {
    pub(crate) fn open(path: &Path) -> Result<Self> {
        let lib = Box::leak(Box::new(libloading::Library::new(path)?));
        let init_tracing: libloading::Symbol<fn(&tracing::Dispatch) -> ()> =
            unsafe { lib.get(b"init_tracing")? };
        tracing::dispatcher::get_default(|dispatch| {
            init_tracing(dispatch);
        });

        Ok(Self {
            path: path.to_owned(),
            lib,
        })
    }

    pub fn get_symbol<T>(&self, symbol: &[u8]) -> Result<libloading::Symbol<T>> {
        unsafe { self.lib.get(symbol).map_err(Error::from) }
    }
}

impl PartialEq for Plugin {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for Plugin {}

impl std::fmt::Debug for Plugin {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Plugin")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}
