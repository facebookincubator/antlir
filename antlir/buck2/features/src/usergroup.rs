/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use crate::types::PathInLayer;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct UserName(String);

impl UserName {
    pub fn name(&self) -> &str {
        &self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct GroupName(String);

impl GroupName {
    pub fn name(&self) -> &str {
        &self.0
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
pub struct Uid(u32);

impl Uid {
    pub fn id(self) -> u32 {
        self.0
    }
}

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
pub struct Gid(u32);

impl Gid {
    pub fn id(self) -> u32 {
        self.0
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct User {
    pub name: UserName,
    pub uid: Option<Uid>,
    pub primary_group: GroupName,
    pub supplementary_groups: Vec<GroupName>,
    pub home_dir: PathInLayer,
    pub shell: PathInLayer,
    pub comment: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Group {
    pub name: GroupName,
    pub gid: Option<Gid>,
}
