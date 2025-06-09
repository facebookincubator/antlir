/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;

pub mod dir_entry;
pub mod rpm;
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
