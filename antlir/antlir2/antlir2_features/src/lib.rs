/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Ordering;
use std::hash::Hash;

use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

pub mod plugin;
pub mod stat;
pub mod types;

#[derive(thiserror::Error, Debug)]
pub enum Error {
    #[error("could not load plugin: {0}")]
    PluginLoad(#[from] libloading::Error),
    #[error("plugin '{0}' was not loaded")]
    PluginNotLoaded(Label),
    #[error("opened plugin but it was bad: {0}")]
    BadPlugin(String),
    #[error("could not deserialize feature json: {0}")]
    Deserialize(serde_json::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct Feature {
    #[serde(deserialize_with = "Label::deserialize_owned")]
    pub label: Label,
    pub feature_type: String,
    pub data: serde_json::Value,
    #[serde(deserialize_with = "Label::deserialize_owned")]
    plugin: Label,
}

impl PartialOrd for Feature {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

// TODO(T177933397): this Ord implementation is inefficient and should be
// removed when we can correctly ban identical features from being included
// multiple times in a single layer.
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

impl Feature {
    pub fn plugin(&self) -> Result<&'static plugin::Plugin> {
        plugin::REGISTRY
            .lock()
            .expect("plugin registry poisoned")
            .get(&self.plugin)
            .copied()
            .ok_or_else(|| Error::PluginNotLoaded(self.plugin.clone()))
    }
}
