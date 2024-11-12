/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
/// Specification of the test runtime (the rootfs layer, environment, etc)
pub(crate) struct RuntimeSpec {
    /// Path to layer to run the test in
    pub(crate) layer: PathBuf,
    /// Run the test as this user
    pub(crate) user: String,
    #[serde(default)]
    /// Set container hostname
    pub(crate) hostname: Option<String>,
    /// Boot the container with /init as pid1 before running the test
    pub(crate) boot: Option<Boot>,
    /// Set these env vars in the test environment
    #[serde(default)]
    pub(crate) setenv: BTreeMap<String, String>,
    #[serde(default)]
    /// Set these env vars in the test environment based on what is present in the parent
    pub(crate) pass_env: Vec<String>,
    #[serde(default)]
    /// Mount runtime platform (aka /usr/local/fbcode) from the host
    pub(crate) mount_platform: bool,
    /// Mounts required by the layer-under-test
    pub(crate) mounts: HashMap<PathBuf, PathBuf>,
    /// Run the test in an unprivileged user namespace
    pub(crate) rootless: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct Boot {
    /// Add Requires= and After= dependencies on these units
    pub(crate) requires_units: Vec<String>,
    /// Add an After= dependency on these units
    pub(crate) after_units: Vec<String>,
    /// Add Wants= dependencies on these units
    pub(crate) wants_units: Vec<String>,
}
