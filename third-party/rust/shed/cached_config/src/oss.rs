/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

use anyhow::Result;
use fbinit::FacebookInit;
use slog::Logger;
use std::{collections::HashMap, path::PathBuf, time::Duration};

use crate::ConfigStore;

macro_rules! fb_unimplemented {
    () => {
        unimplemented!("This is implemented only for fbcode_build!")
    };
}

impl ConfigStore {
    /// # Panics
    /// When called in non-fbcode builds
    pub fn configerator(
        _: FacebookInit,
        _: impl Into<Option<Logger>>,
        _: impl Into<Option<Duration>>,
        _: Duration,
    ) -> Result<Self> {
        fb_unimplemented!()
    }

    /// # Panics
    /// When called in non-fbcode builds
    pub fn signed_configerator(
        _: FacebookInit,
        _: impl Into<Option<Logger>>,
        _: HashMap<String, String>,
        _: impl Into<Option<Duration>>,
        _: Duration,
    ) -> Result<Self> {
        fb_unimplemented!()
    }

    /// # Panics
    /// When called in non-fbcode builds
    pub fn regex_signed_configerator(
        _: FacebookInit,
        _: impl Into<Option<Logger>>,
        _: Vec<(String, String)>,
        _: impl Into<Option<Duration>>,
        _: Duration,
    ) -> Result<Self> {
        fb_unimplemented!()
    }

    /// # Panics
    /// When called in non-fbcode builds
    pub fn materialized_configs(
        _: impl Into<Option<Logger>>,
        _: PathBuf,
        _: impl Into<Option<Duration>>,
    ) -> Self {
        fb_unimplemented!()
    }
}
