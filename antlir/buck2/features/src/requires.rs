/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::types::PathInLayer;
use crate::usergroup::GroupName;
use crate::usergroup::UserName;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Requires {
    #[serde(default)]
    pub files: Vec<PathInLayer>,
    #[serde(default)]
    pub users: Vec<UserName>,
    #[serde(default)]
    pub groups: Vec<GroupName>,
}
