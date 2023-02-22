/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(cow_is_borrowed)]

use std::borrow::Cow;
use std::cmp::Ordering;
use std::cmp::PartialOrd;
use std::hash::Hash;
use std::hash::Hasher;
use std::ops::Range;

use once_cell::sync::Lazy;
use regex::Regex;
use serde::de::Deserializer;
use serde::de::Error as _;
use serde::ser::Serializer;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

static ALLOWED_NAME_CHARSET: &str = r"[a-zA-Z0-9,.=\-/~@!+$_]";
static LABEL_PATTERN: Lazy<String> = Lazy::new(|| {
    format!(
        r"(.+?)//({}*?):({}*)",
        ALLOWED_NAME_CHARSET, ALLOWED_NAME_CHARSET,
    )
});
static LABEL_WITH_CONFIG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(
        r"^{}(?:\s+\({}\))?$",
        *LABEL_PATTERN, *LABEL_PATTERN
    ))
    .expect("I know this works")
});

#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("label '{0}' does not match the regex")]
    NoMatch(String),
    #[error("label config was not a valid label: '{0}'")]
    InvalidConfig(Box<Error>),
}

/// A buck target label. Points to a specific target and is always fully
/// qualified (aka, with cell name).
#[derive(Clone, Eq)]
pub struct Label<'a> {
    full: Cow<'a, str>,
    cell: Range<usize>,
    package: Range<usize>,
    name: Range<usize>,
    config: Option<Box<Label<'a>>>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Parts<'a> {
    cell: &'a str,
    package: &'a str,
    name: &'a str,
    config: Option<Box<Parts<'a>>>,
}

impl<'a> Label<'a> {
    pub fn new(s: impl Into<Cow<'a, str>>) -> Result<Self, Error> {
        let full: Cow<'a, str> = s.into();
        match LABEL_WITH_CONFIG_RE.captures(&full) {
            Some(cap) => {
                assert_eq!(
                    cap.len(),
                    7,
                    "the regex matched, there must be exactly 7 groups"
                );
                let cell = cap.get(1).expect("cell must exist").range();
                let package = cap.get(2).expect("package must exist").range();
                let name = cap.get(3).expect("name must exist").range();
                // If group 4 (the cell of the config) participated in the
                // match, then all the others parts of the config must have as
                // well. This unintuitive condition is necessary because
                // `cap.len()` always returns the total number of groups, even
                // if they didn't participate in the match
                let config = if let Some(cfg_cell) = cap.get(4) {
                    let cfg_cell = cfg_cell.range();
                    let cfg_package = cap.get(5).expect("cfg_package must exist").range();
                    let cfg_name = cap.get(6).expect("cfg_name must exist").range();
                    Some(Box::new(Self {
                        full: full.clone(),
                        cell: cfg_cell,
                        package: cfg_package,
                        name: cfg_name,
                        config: None,
                    }))
                } else {
                    None
                };
                Ok(Self {
                    full,
                    cell,
                    package,
                    name,
                    config,
                })
            }
            None => Err(Error::NoMatch(full.to_string())),
        }
    }

    pub fn parts(&'a self) -> Parts<'a> {
        Parts {
            cell: self.cell(),
            package: self.package(),
            name: self.name(),
            config: self.config().map(Label::parts).map(Box::new),
        }
    }

    /// Escape the Label to be used in a filename. This flattens the label space
    /// so that a directory hierarchy does not need to be created to match the
    /// repo structure (in other words, '/' and ' ' get replaced with '-').
    pub fn flat_filename(&self) -> String {
        self.to_string().replace(['/', ' '], "-")
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

    pub fn config(&self) -> Option<&Label> {
        self.config.as_deref()
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

    pub fn to_owned(&self) -> Label<'static> {
        Label {
            full: Cow::Owned(self.full.to_string()),
            cell: self.cell.clone(),
            package: self.package.clone(),
            name: self.name.clone(),
            config: self.config.as_deref().map(|b| Box::new(b.to_owned())),
        }
    }
}

impl<'a, 'b> PartialEq<Label<'b>> for Label<'a> {
    fn eq(&self, other: &Label<'b>) -> bool {
        self.parts() == other.parts()
    }
}

impl<'a, 'b> PartialOrd<Label<'b>> for Label<'a> {
    fn partial_cmp(&self, other: &Label<'b>) -> Option<Ordering> {
        self.parts().partial_cmp(&other.parts())
    }
}

impl<'a> Ord for Label<'a> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.parts().cmp(&other.parts())
    }
}

impl<'a> Hash for Label<'a> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.parts().hash(state);
    }
}

impl<'a> std::str::FromStr for Label<'a> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Self::new(s.to_owned())
    }
}

impl<'a> std::fmt::Debug for Label<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("Label").field(&self.to_string()).finish()
    }
}

impl<'a> std::fmt::Display for Label<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.config {
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
                write!(f, "{}//{}:{}", self.cell(), self.package(), self.name(),)
            }
        }
    }
}

/// This simple CowWrapper struct with #[serde(borrow)] makes it easy to
/// deserialize a `Label` using borrowed or owned data as appropriate, without
/// having to figure it out ourselves. See [tests::serde] for proof.
#[derive(Deserialize, Serialize)]
struct CowWrapper<'a>(#[serde(borrow)] Cow<'a, str>);

impl<'de: 'a, 'a> Deserialize<'de> for Label<'a> {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let c = CowWrapper::deserialize(deserializer)?;
        Label::new(c.0).map_err(D::Error::custom)
    }
}

impl<'a> Serialize for Label<'a> {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(Label: Send, Sync);

    #[test]
    fn cow_behavior() {
        let l =
            Label::new("abc//path/to/target:label (config//path/to:config)").expect("valid label");
        assert!(
            l.full.is_borrowed(),
            "static str should produce borrowed Label"
        );
        assert!(
            l.config.as_ref().expect("has config").full.is_borrowed(),
            "static str should produce borrowed config Label"
        );

        let l = Label::new("abc//path/to/target:label (config//path/to:config)".to_string())
            .expect("valid label");
        assert!(
            l.full.is_owned(),
            "passing owned String should produce owned Label"
        );
        assert!(
            l.config.as_ref().expect("has config").full.is_owned(),
            "passing owned String should produce owned config Label"
        );
    }

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
                config: Some(Box::new(Parts {
                    cell: "config",
                    package: "path/to",
                    name: "config",
                    config: None
                })),
            },
            Label::new("abc//path/to/target:label (config//path/to:config)")
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
            Err(Error::NoMatch(s.into())),
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
    fn display(#[case] s: &str) {
        let label = Label::new(s).expect("well-formed");
        assert_eq!(s, label.to_string());
    }

    #[test]
    fn escape() {
        let label: Label = Label::new("abc//path/to/target:label").expect("well-formed");
        assert_eq!("abc--path-to-target:label", label.flat_filename());
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
        assert!(
            label.full.is_borrowed(),
            "serde_json::from_str should produce borrowed Label"
        );
        let mut deser =
            serde_json::Deserializer::from_reader(&br#""abc//path/to/target:label""#[..]);
        let label = Label::deserialize(&mut deser).expect("well formed");
        assert!(
            label.full.is_owned(),
            "serde_json::from_reader should produce owned Label"
        );

        // serialization is easier to check
        assert_eq!(
            r#""abc//path/to/target:label""#,
            serde_json::to_string(&label).expect("infallible")
        );
    }
}
