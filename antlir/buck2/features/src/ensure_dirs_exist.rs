/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::stat::Mode;
use crate::types::PathInLayer;
use crate::usergroup::GroupName;
use crate::usergroup::UserName;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct EnsureDirsExist {
    pub group: GroupName,
    pub into_dir: PathInLayer,
    pub mode: Mode,
    pub subdirs_to_create: PathInLayer,
    pub user: UserName,
}
