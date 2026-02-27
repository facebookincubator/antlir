/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use bon::Builder;
use serde::Deserialize;
use serde::Serialize;

use super::Fact;
use super::Key;
use crate::fact_impl;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize, Builder)]
#[builder(on(String, into))]
pub struct Deb {
    name: String,
    version: String,
    arch: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    source: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    installed_size: Option<u64>,
}

#[fact_impl("antlir2_facts::fact::deb::Deb")]
impl Fact for Deb {
    fn key(&self) -> Key {
        format!("{}:{}:{}", self.name, self.version, self.arch).into()
    }
}

impl Deb {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }

    pub fn arch(&self) -> &str {
        &self.arch
    }

    pub fn source(&self) -> Option<&str> {
        self.source.as_deref()
    }

    pub fn installed_size(&self) -> Option<u64> {
        self.installed_size
    }
}

impl std::fmt::Display for Deb {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}:{}:{}", self.name, self.version, self.arch)
    }
}
