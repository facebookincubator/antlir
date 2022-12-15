/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::stat::Mode;
use crate::types::BuckOutSource;
use crate::types::PathInLayer;
use crate::usergroup::GroupName;
use crate::usergroup::UserName;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Install {
    pub dst: PathInLayer,
    pub group: GroupName,
    pub mode: Option<Mode>,
    pub src: BuckOutSource,
    pub user: UserName,
}
