/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use std::path::PathBuf;

use serde::Deserialize;

#[derive(Debug, Deserialize, PartialEq, Eq)]
pub struct target_t {
    pub name: String,
    pub path: PathBuf,
}
