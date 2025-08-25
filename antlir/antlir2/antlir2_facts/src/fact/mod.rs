/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;

use serde::Deserialize;
use serde::Serialize;

pub mod dir_entry;
pub mod rpm;
pub mod subvolume;
pub mod systemd_unit;
pub mod user;

use super::Key;

#[typetag::serde(tag = "type", content = "value")]
pub trait Fact: Any {
    fn key(&self) -> Key;
}

static_assertions::assert_obj_safe!(Fact);

pub trait FactKind {
    const KIND: &'static str;
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Generic {
    #[serde(rename = "type")]
    ty: String,
    key: String,
    value: serde_json::Value,
}

impl Generic {
    pub fn ty(&self) -> &str {
        &self.ty
    }

    pub fn key(&self) -> Key {
        self.key.clone().into()
    }

    pub fn value(&self) -> &serde_json::Value {
        &self.value
    }
}
