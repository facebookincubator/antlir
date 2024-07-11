/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::fmt::Debug;
use std::path::PathBuf;

use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use similar::TextDiff;

use crate::file_entry::FileEntry;
use crate::file_entry::NameOrId;
use crate::file_entry::XattrData;
use crate::rpm_entry::RpmEntry;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub(crate) struct LayerDiff {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) file: Option<BTreeMap<PathBuf, FileDiff>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) rpm: Option<BTreeMap<String, RpmDiff>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", tag = "op", content = "diff")]
pub(crate) enum FileDiff {
    Added(FileEntry),
    Removed(FileEntry),
    Diff(FileEntryDiff),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", tag = "op", content = "diff")]
pub(crate) enum RpmDiff {
    Installed(RpmEntry),
    Removed(RpmEntry),
    Changed(RpmEntryDiff),
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
struct FieldDiff<T> {
    parent: T,
    child: T,
}

impl<T> FieldDiff<T> {
    fn new(parent: T, child: T) -> Option<Self>
    where
        T: PartialEq,
    {
        if parent == child {
            None
        } else {
            Some(Self { parent, child })
        }
    }
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct FileEntryDiff {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text_patch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user: Option<FieldDiff<NameOrId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group: Option<FieldDiff<NameOrId>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<FieldDiff<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file_type: Option<FieldDiff<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_hash: Option<FieldDiff<String>>,
    #[serde(default, skip_serializing_if = "XattrDiff::is_empty")]
    xattrs: XattrDiff,
}

impl FileEntryDiff {
    #[deny(unused_variables)]
    pub(crate) fn new(parent: &FileEntry, child: &FileEntry) -> Self {
        let FileEntry {
            mode,
            user,
            group,
            file_type,
            text,
            content_hash,
            xattrs,
        } = parent;
        let text = match text.as_deref() {
            Some(parent_text) => {
                let text_diff = TextDiff::from_lines(
                    parent_text,
                    child
                        .text
                        .as_deref()
                        .expect("something is terribly wrong if we went from text to binary"),
                );
                Some(
                    text_diff
                        .unified_diff()
                        .context_radius(3)
                        .header("parent", "child")
                        .to_string(),
                )
            }
            None => None,
        };
        Self {
            mode: FieldDiff::new(mode.to_string(), child.mode.to_string()),
            user: FieldDiff::new(user.clone(), child.user.clone()),
            group: FieldDiff::new(group.clone(), child.group.clone()),
            file_type: FieldDiff::new(file_type.to_string(), child.file_type.to_string()),
            content_hash: if text.is_some() {
                None
            } else {
                FieldDiff::new(
                    content_hash.clone().expect("set if text is None"),
                    child.content_hash.clone().expect("set if text is None"),
                )
            },
            xattrs: XattrDiff::new(xattrs, &child.xattrs),
            text_patch: text,
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
struct XattrDiff {
    removed: BTreeSet<String>,
    added: BTreeMap<String, XattrData>,
    changed: BTreeMap<String, XattrData>,
}

impl XattrDiff {
    fn new(parent: &BTreeMap<String, XattrData>, child: &BTreeMap<String, XattrData>) -> Self {
        let mut s = Self::default();
        for (key, val) in parent {
            match child.get(key) {
                Some(c) => {
                    if c != val {
                        s.changed.insert(key.clone(), c.clone());
                    }
                }
                None => {
                    s.removed.insert(key.clone());
                }
            }
        }
        for (key, val) in child {
            if !parent.contains_key(key) {
                s.added.insert(key.clone(), val.clone());
            }
        }
        s
    }

    fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.added.is_empty() && self.changed.is_empty()
    }
}

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct RpmEntryDiff {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    evra: Option<FieldDiff<String>>,
}

impl RpmEntryDiff {
    pub(crate) fn new(parent: &RpmEntry, child: &RpmEntry) -> Self {
        Self {
            evra: FieldDiff::new(parent.evra.clone(), child.evra.clone()),
        }
    }
}
