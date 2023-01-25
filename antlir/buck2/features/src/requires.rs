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
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Requires<'a> {
    #[serde(default)]
    pub files: Vec<PathInLayer<'a>>,
    #[serde(default)]
    pub users: Vec<UserName<'a>>,
    #[serde(default)]
    pub groups: Vec<GroupName<'a>>,
}
