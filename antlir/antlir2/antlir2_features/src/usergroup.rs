/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use serde::Deserialize;
use serde::Serialize;

use crate::types::PathInLayer;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct UserName<'a>(Cow<'a, str>);

impl<'a> UserName<'a> {
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl<'a, S> From<S> for UserName<'a>
where
    S: Into<Cow<'a, str>>,
{
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct GroupName<'a>(Cow<'a, str>);

impl<'a> GroupName<'a> {
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl<'a, S> From<S> for GroupName<'a>
where
    S: Into<Cow<'a, str>>,
{
    fn from(s: S) -> Self {
        Self(s.into())
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
#[serde(bound(deserialize = "'de: 'a"))]
pub struct User<'a> {
    pub name: UserName<'a>,
    pub uid: Option<Uid>,
    pub primary_group: GroupName<'a>,
    pub supplementary_groups: Vec<GroupName<'a>>,
    pub home_dir: PathInLayer<'a>,
    pub shell: PathInLayer<'a>,
    pub comment: Option<Cow<'a, str>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct UserMod<'a> {
    pub username: UserName<'a>,
    pub add_supplementary_groups: Vec<GroupName<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Group<'a> {
    pub name: GroupName<'a>,
    pub gid: Option<Gid>,
}
