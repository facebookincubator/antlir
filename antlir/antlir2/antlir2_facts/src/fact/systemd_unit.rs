/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_systemd::UnitFile;

use super::Fact;
use super::Key;
use crate::fact_impl;

#[fact_impl("antlir2_systemd::UnitFile")]
impl Fact for UnitFile {
    fn key(&self) -> Key {
        self.name().as_bytes().into()
    }
}
