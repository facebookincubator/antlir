/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::HashMap;
use std::path::Path;

use buck_label::Label;
use buck_version::BuckVersion;
use serde::Deserialize;
use serde::Serialize;

/// On buck1, this is the only means by which we can track dependencies. JSON
/// input comes in with Buck target labels, and [TargetsAndOutputs] is used to
/// inform the compiler of where the built artifacts can be found on disk.
/// Buck2 can manage the depgraph better within buck itself, so we can pass
/// locations directly without needing this indirection.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct TargetsAndOutputs<'a> {
    #[serde(borrow)]
    metadata: Metadata<'a>,
    #[serde(borrow)]
    targets_and_outputs: HashMap<Label, Cow<'a, Path>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Metadata<'a> {
    buck_version: BuckVersion,
    #[serde(borrow)]
    default_cell: Cow<'a, str>,
}

impl<'a> Metadata<'a> {
    pub fn new(buck_version: BuckVersion, default_cell: impl Into<Cow<'a, str>>) -> Self {
        Self {
            buck_version,
            default_cell: default_cell.into(),
        }
    }
}

impl<'a> TargetsAndOutputs<'a> {
    pub fn new(metadata: Metadata<'a>, targets_and_outputs: HashMap<Label, Cow<'a, Path>>) -> Self {
        Self {
            metadata,
            targets_and_outputs,
        }
    }

    pub fn default_cell(&self) -> &str {
        &self.metadata.default_cell
    }

    /// Give the (relative) path of an output file.
    pub fn path(&self, target: &'a Label) -> Option<Cow<'a, Path>> {
        self.targets_and_outputs.get(target).cloned()
    }

    pub fn len(&self) -> usize {
        self.targets_and_outputs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.targets_and_outputs.is_empty()
    }

    pub fn iter(&'a self) -> std::collections::hash_map::Iter<'a, Label, Cow<'a, Path>> {
        self.targets_and_outputs.iter()
    }
}

impl<'a> IntoIterator for TargetsAndOutputs<'a> {
    type IntoIter = <HashMap<Label, Cow<'a, Path>> as IntoIterator>::IntoIter;
    type Item = (Label, Cow<'a, Path>);

    fn into_iter(self) -> Self::IntoIter {
        self.targets_and_outputs.into_iter()
    }
}
