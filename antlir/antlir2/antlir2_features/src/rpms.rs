/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use serde::de::Error;
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case", bound(deserialize = "'de: 'a"))]
pub enum Source<'a> {
    Subject(Cow<'a, str>),
    #[serde(rename = "src")]
    Source(BuckOutSource<'a>),
    #[serde(rename = "subjects_src")]
    SubjectsSource(BuckOutSource<'a>),
}

/// Buck2's `record` will always include `null` values, but serde's native enum
/// deserialization will fail if there are multiple keys, even if the others are
/// null.
/// TODO(vmagro): make this general in the future (either codegen from `record`s
/// or as a proc-macro)
impl<'a, 'de: 'a> Deserialize<'de> for Source<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(bound(deserialize = "'de: 'a"))]
        struct SourceStruct<'a> {
            subject: Option<Cow<'a, str>>,
            src: Option<BuckOutSource<'a>>,
            subjects_src: Option<BuckOutSource<'a>>,
        }

        SourceStruct::deserialize(deserializer).and_then(|s| {
            match (s.subject, s.src, s.subjects_src) {
                (Some(subj), None, None) => Ok(Self::Subject(subj)),
                (None, Some(source), None) => Ok(Self::Source(source)),
                (None, None, Some(subjects_src)) => Ok(Self::SubjectsSource(subjects_src)),
                _ => Err(D::Error::custom(
                    "exactly one of {subject, src, subjects_src} must be set",
                )),
            }
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Item<'a> {
    pub action: Action,
    pub rpm: Source<'a>,
}

#[derive(
    Debug,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize,
    Default
)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct Rpm<'a> {
    pub items: Vec<Item<'a>>,
    #[serde(skip_deserializing)]
    pub internal_only_options: InternalOnlyOptions,
}

#[derive(
    Debug,
    Default,
    Clone,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deserialize,
    Serialize
)]
pub struct InternalOnlyOptions {
    #[serde(default)]
    pub ignore_postin_script_error: bool,
}
