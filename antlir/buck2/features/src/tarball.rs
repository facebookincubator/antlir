/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::types::BuckOutSource;
use crate::types::PathInLayer;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Tarball {
    pub src: BuckOutSource,
    pub into_dir: PathInLayer,
    pub force_root_ownership: bool,
}
