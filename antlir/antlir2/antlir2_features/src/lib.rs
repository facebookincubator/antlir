/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Ordering;
use std::hash::Hash;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use buck_label::Label;
use once_cell::sync::OnceCell;
use serde::Deserialize;
use serde::Serialize;

pub mod stat;
pub mod types;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("run_info was malformed")]
    RunInfo,
    #[error("could not load plugin: {0}")]
    PluginLoad(#[from] libloading::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Feature {
    #[serde(deserialize_with = "Label::deserialize_owned")]
    pub label: Label,
    pub feature_type: String,
    pub data: serde_json::Value,
    #[serde(rename = "plugin")]
    plugin_json: PluginJson,
    #[serde(skip)]
    plugin: OnceCell<Arc<Plugin>>,
}

// TODO(T177933397): this hash implementation is inefficient and should be
// removed when we can correctly ban identical features from being included
// multiple times in a single layer.
impl Hash for Feature {
    #[deny(unused_variables)]
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        let Self {
            label,
            feature_type,
            data,
            plugin_json,
            plugin: _,
        } = self;
        label.hash(state);
        feature_type.hash(state);
        data.to_string().hash(state);
        plugin_json.hash(state);
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
struct PluginJson {
    plugin: PathBuf,
    libs: PathBuf,
}

pub struct Plugin {
    path: PathBuf,
    lib: libloading::Library,
}

impl Feature {
    pub fn plugin(&self) -> Result<&Plugin> {
        if let Some(plugin) = &self.plugin.get() {
            Ok(plugin)
        } else {
            let plugin = Plugin::open(&self.plugin_json.plugin)?;
            let plugin = match self.plugin.try_insert(Arc::new(plugin)) {
                Ok(plugin) => plugin,
                Err((plugin, _)) => plugin,
            };
            Ok(plugin)
        }
    }
}

impl Plugin {
    fn open(path: &Path) -> Result<Self> {
        let lib = libloading::Library::new(path)?;
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

impl PartialOrd for Feature {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Feature {
    fn cmp(&self, other: &Self) -> Ordering {
        match self.label.cmp(&other.label) {
            Ordering::Equal => match self.feature_type.cmp(&other.feature_type) {
                Ordering::Equal => serde_json::to_string(&self.data)
                    .expect("failed to serialize")
                    .cmp(&serde_json::to_string(&other.data).expect("failed to serialize")),
                ord => ord,
            },
            ord => ord,
        }
    }
}
