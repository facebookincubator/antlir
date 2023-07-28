/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::ffi::OsString;
use std::fmt::Debug;
use std::path::PathBuf;

use serde::de::Visitor;
use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use similar::TextDiff;

use crate::entry::Entry;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(transparent)]
pub(crate) struct LayerDiff(pub(crate) BTreeMap<PathBuf, Diff>);

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case", tag = "op", content = "diff")]
pub(crate) enum Diff {
    Added(Entry),
    Removed(Entry),
    Diff(EntryDiff),
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
pub(crate) struct EntryDiff {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    text_patch: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    user: Option<FieldDiff<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    group: Option<FieldDiff<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    mode: Option<FieldDiff<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    file_type: Option<FieldDiff<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    content_hash: Option<FieldDiff<String>>,
    #[serde(default, skip_serializing_if = "XattrDiff::is_empty")]
    xattrs: XattrDiff,
}

impl EntryDiff {
    #[deny(unused_variables)]
    pub(crate) fn new(parent: &Entry, child: &Entry) -> Self {
        let Entry {
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

#[derive(Clone, PartialEq, Eq)]
struct XattrData(Vec<u8>);

impl Serialize for XattrData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match std::str::from_utf8(&self.0) {
            Ok(text) => serializer.serialize_str(text),
            Err(_) => self.0.serialize(serializer),
        }
    }
}

impl Debug for XattrData {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match std::str::from_utf8(&self.0) {
            Ok(text) => f.debug_tuple("XattrData").field(&text).finish(),
            Err(_) => f.debug_tuple("XattrData").field(&self.0).finish(),
        }
    }
}

struct XattrDataVisitor;

impl<'de> Visitor<'de> for XattrDataVisitor {
    type Value = XattrData;

    fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
        formatter.write_str("a string or byte array")
    }

    fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(XattrData(v.into_bytes()))
    }

    fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(XattrData(v.as_bytes().to_vec()))
    }

    fn visit_byte_buf<E>(self, v: Vec<u8>) -> Result<Self::Value, E>
    where
        E: serde::de::Error,
    {
        Ok(XattrData(v))
    }
}

impl<'de> Deserialize<'de> for XattrData {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        deserializer.deserialize_str(XattrDataVisitor)
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Deserialize, Serialize)]
struct XattrDiff {
    removed: BTreeSet<OsString>,
    added: BTreeMap<OsString, XattrData>,
    changed: BTreeMap<OsString, XattrData>,
}

impl XattrDiff {
    fn new(parent: &BTreeMap<OsString, Vec<u8>>, child: &BTreeMap<OsString, Vec<u8>>) -> Self {
        let mut s = Self::default();
        for (key, val) in parent {
            match child.get(key) {
                Some(c) => {
                    if c != val {
                        s.changed.insert(key.clone(), XattrData(c.clone()));
                    }
                }
                None => {
                    s.removed.insert(key.clone());
                }
            }
        }
        for (key, val) in child {
            if !parent.contains_key(key) {
                s.added.insert(key.clone(), XattrData(val.clone()));
            }
        }
        s
    }

    fn is_empty(&self) -> bool {
        self.removed.is_empty() && self.added.is_empty() && self.changed.is_empty()
    }
}
