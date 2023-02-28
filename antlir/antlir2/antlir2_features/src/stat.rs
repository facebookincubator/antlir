/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

#[derive(
    Debug,
    Copy,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
#[serde(transparent)]
#[repr(transparent)]
pub struct Mode(pub u32);

impl From<u32> for Mode {
    fn from(u: u32) -> Self {
        Self(u)
    }
}

impl Mode {
    pub fn as_raw(&self) -> u32 {
        self.0
    }
}
