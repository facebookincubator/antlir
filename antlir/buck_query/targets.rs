/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;

use serde::Deserialize;
use serde::Serialize;

// Simple macro to avoid repeating code that is exactly the same for every
// variant of the Target enum
#[macro_export]
macro_rules! target_common {
    ($x:ident, $t:ident, $c:block) => {
        match $x {
            Target::RustLibrary($t) => $c,
            Target::RustBinary($t) => $c,
            Target::CxxLibrary($t) => $c,
            Target::CxxBinary($t) => $c,
        }
    };
    ($x:ident, $t:ident, $c:expr) => {
        target_common!($x, $t, { $c })
    };
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(tag = "buck.type")]
pub enum Target {
    #[serde(rename = "rust_library")]
    RustLibrary(RustLibrary),
    #[serde(rename = "rust_binary")]
    RustBinary(RustBinary),
    #[serde(rename = "cxx_library")]
    CxxLibrary(CxxLibrary),
    #[serde(rename = "cxx_binary")]
    CxxBinary(CxxBinary),
}

impl Target {
    pub fn name(&self) -> &str {
        target_common!(self, target, { &target.common.common.name })
    }

    pub fn fully_qualified_name(&self) -> &str {
        target_common!(self, target, { &target.common.common.fully_qualified_name })
    }

    pub fn labels(&self) -> &BTreeSet<String> {
        target_common!(self, target, &target.common.common.labels)
    }

    pub fn base_path(&self) -> &str {
        target_common!(self, target, &target.common.common.base_path)
    }
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct TargetCommon {
    #[serde(skip_serializing)]
    pub fully_qualified_name: String,
    pub name: String,
    pub srcs: Vec<String>,
    pub deps: Vec<String>,
    #[serde(skip_serializing)]
    pub labels: BTreeSet<String>,
    pub licenses: Vec<String>,
    #[serde(rename = "buck.base_path", skip_serializing)]
    pub base_path: String,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RustCommon {
    #[serde(flatten)]
    pub common: TargetCommon,
    #[serde(rename = "crate")]
    pub crate_: Option<String>,
    pub crate_root: String,
    pub edition: Option<String>,
    pub features: Vec<String>,
    pub mapped_srcs: BTreeMap<String, String>,
    pub named_deps: BTreeMap<String, String>,
    pub rustc_flags: Vec<String>,
    pub env: BTreeMap<String, String>,
    pub unittests: bool,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RustLibrary {
    #[serde(flatten)]
    pub common: RustCommon,
    pub proc_macro: bool,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct RustBinary {
    #[serde(flatten)]
    pub common: RustCommon,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct CxxCommon {
    #[serde(flatten)]
    pub common: TargetCommon,
    pub headers: serde_json::Value,
    pub header_namespace: Option<String>,
    pub include_directories: Vec<String>,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct CxxLibrary {
    #[serde(flatten)]
    pub common: CxxCommon,
    pub exported_headers: serde_json::Value,
}

#[derive(Debug, Default, Deserialize, Serialize)]
#[serde(default)]
pub struct CxxBinary {
    #[serde(flatten)]
    pub common: CxxCommon,
}
