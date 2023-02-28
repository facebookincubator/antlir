/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

use crate::types::BuckOutSource;

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
#[serde(rename_all = "snake_case")]
pub enum Action {
    Install,
    RemoveIfExists,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "snake_case", bound(deserialize = "'de: 'a"))]
pub enum Source<'a> {
    Name(Cow<'a, str>),
    Source(BuckOutSource<'a>),
}

/// The RPM action format is pretty hairy, clean this up at some point to have a
/// nicer, more Rusty/safe structure
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Rpm<'a> {
    pub action: Action,
    pub rpms: Vec<Source<'a>>,
}

/// A new definition of the Rpm feature for antlir2. This is currently a
/// "virtual" feature in that it is not expressed in the buck2 target graph, but
/// is constructed from multiple [Rpm] feature objects while building up the
/// depgraph for an antlir2 build.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Rpm2<'a> {
    pub items: Vec<Rpm2Item<'a>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Rpm2Item<'a> {
    pub action: Action,
    pub rpms: Vec<Source<'a>>,
    pub label: Label<'a>,
}
