/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

pub mod dir_entry;
pub mod user;

pub trait Fact<'a, 'de>: Serialize + Deserialize<'de> {
    type Key: AsRef<[u8]>;

    fn kind() -> &'static str {
        std::any::type_name::<Self>()
    }

    fn key(&'a self) -> Self::Key;
}
