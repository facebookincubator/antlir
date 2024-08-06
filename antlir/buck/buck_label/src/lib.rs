/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(cow_is_borrowed)]

use std::cmp::Ordering;
use std::cmp::PartialOrd;
use std::hash::Hash;
use std::hash::Hasher;
use std::ops::Deref;
use std::ops::Range;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::de::Error as _;
use serde::ser::Serializer;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use thiserror::Error;

static ALLOWED_NAME_CHARSET: &str = r"[a-zA-Z0-9,.=\-/~@!+$_#]";
static LABEL_PATTERN: Lazy<String> = Lazy::new(|| {
    format!(
        r"(.+?)//({ALLOWED_NAME_CHARSET}*?):({ALLOWED_NAME_CHARSET}*(?:\[{ALLOWED_NAME_CHARSET}+\])?)",
    )
});
static LABEL_WITH_CONFIG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(r"^{}(?:\s+\((.*)\))?$", *LABEL_PATTERN,)).expect("I know this works")
});

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("label '{0}' does not match the regex: '{1}")]
    NoMatch(String, String),
    #[error("label config was not a valid config: '{0}'")]
    InvalidConfig(Box<Error>),
}

/// A buck target label. Points to a specific target and is always fully
/// qualified (aka, with cell name).
#[derive(Clone, Eq)]
pub struct Label {
    full: String,
    cell: Range<usize>,
    package: Range<usize>,
    name: Range<usize>,
    config: Option<Range<usize>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Parts<'a> {
    cell: &'a str,
    package: &'a str,
    name: &'a str,
    config: Option<&'a str>,
}

impl<'a> Label {
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let full: String = s.into();
        match LABEL_WITH_CONFIG_RE.captures(&full) {
            Some(cap) => {
                assert_eq!(
                    cap.len(),
                    5,
                    "the regex matched, there must be exactly 5 groups"
                );
                let cell = cap.get(1).expect("cell must exist").range();
                let package = cap.get(2).expect("package must exist").range();
                let name = cap.get(3).expect("name must exist").range();
                let config = match cap.get(4) {
                    Some(m) => match m.as_str() {
                        "<unspecified>" => None,
                        _ => Some(m.range()),
                    },
                    None => None,
                };
                Ok(Self {
                    full: full.to_owned(),
                    cell,
                    package,
                    name,
                    config,
                })
            }
            None => Err(Error::NoMatch(full, LABEL_WITH_CONFIG_RE.to_string())),
        }
    }

    pub fn parts(&'a self) -> Parts<'a> {
        Parts {
            cell: self.cell(),
            package: self.package(),
            name: self.name(),
            config: self.config(),
        }
    }

    pub fn cell(&self) -> &str {
        &self.full[self.cell.clone()]
    }

    pub fn package(&self) -> &str {
        &self.full[self.package.clone()]
    }

    pub fn name(&self) -> &str {
        &self.full[self.name.clone()]
    }

    pub fn config(&self) -> Option<&str> {
        match &self.config {
            Some(rng) => Some(&self.full[rng.clone()]),
            None => None,
        }
    }

    pub fn as_unconfigured(&self) -> Self {
        Self {
            full: self.full.clone(),
            cell: self.cell.clone(),
            package: self.package.clone(),
            name: self.name.clone(),
            config: None,
        }
    }

    pub fn to_owned(&self) -> Label {
        Label {
            full: self.full.clone(),
            cell: self.cell.clone(),
            package: self.package.clone(),
            name: self.name.clone(),
            config: self.config.clone(),
        }
    }
}

impl PartialEq<Label> for Label {
    fn eq(&self, other: &Label) -> bool {
        self.parts() == other.parts()
    }
}

impl PartialOrd<Label> for Label {
    fn partial_cmp(&self, other: &Label) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Label {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts().cmp(&other.parts())
    }
}

impl Hash for Label {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.parts().hash(state);
    }
}

impl std::str::FromStr for Label {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Self::new(s.to_owned())
    }
}

impl std::fmt::Debug for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("Label").field(&self.to_string()).finish()
    }
}

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.config() {
            Some(cfg) => {
                write!(
                    f,
                    "{}//{}:{} ({cfg})",
                    self.cell(),
                    self.package(),
                    self.name(),
                )
            }
            None => {
                write!(f, "{}//{}:{}", self.cell(), self.package(), self.name())
            }
        }
    }
}

impl<'de> Deserialize<'de> for Label {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Label::new(s).map_err(D::Error::custom)
    }
}

impl Label {
    pub fn deserialize_owned<'de, D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Label::new(s).map_err(D::Error::custom)
    }
}

impl Serialize for Label {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(self.as_ref())
    }
}

impl Deref for Label {
    type Target = str;

    fn deref(&self) -> &Self::Target {
        &self.full
    }
}

impl AsRef<str> for Label {
    fn as_ref(&self) -> &str {
        &self.full
    }
}

impl AsRef<std::ffi::OsStr> for Label {
    fn as_ref(&self) -> &std::ffi::OsStr {
        self.full.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(Label: Send, Sync);

    #[test]
    fn parse_label() {
        assert_eq!(
            Parts {
                cell: "abc",
                package: "path/to/target",
                name: "label",
                config: None,
            },
            Label::new("abc//path/to/target:label")
                .expect("valid label")
                .parts(),
        );
        assert_eq!(
            Parts {
                cell: "abc",
                package: "path/to/target",
                name: "label",
                config: Some("config//path/to:config"),
            },
            Label::new("abc//path/to/target:label (config//path/to:config)")
                .expect("valid label")
                .parts(),
        );
        assert_eq!(
            Parts {
                cell: "abc",
                package: "path/to/target",
                name: "label[subtarget]",
                config: None,
            },
            Label::new("abc//path/to/target:label[subtarget]")
                .expect("valid label")
                .parts(),
        );
        assert_eq!(
            Parts {
                cell: "abc",
                package: "path/to/target",
                name: "label",
                config: Some("cfg:modifier"),
            },
            Label::new("abc//path/to/target:label (cfg:modifier)")
                .expect("valid label")
                .parts(),
        );
    }

    #[test]
    fn anon() {
        assert_eq!(
            Parts {
                cell: "abc",
                package: "path/to/target",
                name: "label",
                config: None,
            },
            Label::new("abc//path/to/target:label (<unspecified>)")
                .expect("valid label")
                .parts(),
        );
    }

    #[rstest]
    #[case::no_cell("//path/to/target:label")]
    #[case::no_colon("abc//path/to/target/label")]
    #[case::double_colon("abc//path/to/target::label")]
    fn bad_labels(#[case] s: &str) {
        assert_eq!(
            Err(Error::NoMatch(s.into(), LABEL_WITH_CONFIG_RE.to_string())),
            Label::new(s),
            "'{}' should not have parsed",
            s
        );
    }

    /// The Display impl should produce the same input when given a well-formed
    /// label
    #[rstest]
    #[case::raw("abc//path/to/target:label")]
    #[case::with_cfg("abc//path/to/target:label (config//path/to:config)")]
    #[case::subtarget("abc//path/to/target:label[foo] (config//path/to:config)")]
    fn display(#[case] s: &str) {
        let label = Label::new(s).expect("well-formed");
        assert_eq!(s, label.to_string());
    }

    #[test]
    fn as_unconfigured() {
        let label =
            Label::new("abc//path/to/target:label (config//path/to:config)").expect("well-formed");
        assert_eq!(
            "abc//path/to/target:label",
            label.as_unconfigured().to_string()
        );
    }

    #[test]
    fn serde() {
        let label: Label =
            serde_json::from_str(r#""abc//path/to/target:label""#).expect("well formed");
        assert_eq!(
            Parts {
                cell: "abc",
                package: "path/to/target",
                name: "label",
                config: None,
            },
            label.parts()
        );
        let mut deser =
            serde_json::Deserializer::from_reader(&br#""abc//path/to/target:label""#[..]);
        let label = Label::deserialize(&mut deser).expect("well formed");
        // serialization is easier to check
        assert_eq!(
            r#""abc//path/to/target:label""#,
            serde_json::to_string(&label).expect("infallible")
        );
    }
}
