/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::any::Any;

use serde::de::DeserializeOwned;
use serde::Serialize;

pub mod dir_entry;
pub mod rpm;
pub mod systemd;
pub mod user;

use super::Key;

pub trait Fact: Any + Serialize + DeserializeOwned {
    fn kind() -> &'static str
    where
        Self: Sized,
    {
        std::any::type_name::<Self>()
    }

    fn key(&self) -> Key;
}
