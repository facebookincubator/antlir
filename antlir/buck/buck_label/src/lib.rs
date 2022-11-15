/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
        r"(?P<cell>.+?)//(?P<package>{}*?):(?P<name>{}+)",
        ALLOWED_NAME_CHARSET, ALLOWED_NAME_CHARSET,
    )
});
static LABEL_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(&format!("^{}$", *LABEL_PATTERN)).expect("I know this works"));
static LABEL_WITH_CONFIG_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(&format!(r"^{}\s+\((?P<cfg>.+)\)$", *LABEL_PATTERN)).expect("I know this works")
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Label {
    cell: String,
    package: String,
    name: String,
    config: Option<Box<Label>>,
}

impl Label {
    pub fn new(s: &str) -> Result<Self, Error> {
        match LABEL_WITH_CONFIG_RE.captures(s) {
            Some(cap) => {
                let mut label = Self::from_caps(&cap);
                let config = Self::new(cap.name("cfg").expect("cfg must exist").as_str())
                    .map_err(|e| Error::InvalidConfig(Box::new(e)))?;
                label.config = Some(Box::new(config));
                Ok(label)
            }
            None => match LABEL_RE.captures(s) {
                Some(cap) => Ok(Self::from_caps(&cap)),
                None => Err(Error::NoMatch(s.to_owned())),
            },
        }
    }

    fn from_caps(cap: &regex::Captures) -> Self {
        let cell = cap.name("cell").expect("cell must exist");
        let package = cap.name("package").expect("package must exist");
        let name = cap.name("name").expect("name must exist");
        Self {
            cell: cell.as_str().to_owned(),
            package: package.as_str().to_owned(),
            name: name.as_str().to_owned(),
            config: None,
        }
    }

    /// Escape the Label to be used in a filename. This flattens the label space
    /// so that a directory hierarchy does not need to be created to match the
    /// repo structure (in other words, '/' gets replaced).
    pub fn flat_filename(&self) -> String {
        self.to_string().replace('/', "-")
    }

    pub fn cell(&self) -> &str {
        &self.cell
    }

    pub fn package(&self) -> &str {
        &self.package
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn config(&self) -> Option<&Label> {
        self.config.as_deref()
    }
}

impl std::str::FromStr for Label {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Self::new(s)
    }
}

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match &self.config {
            Some(cfg) => {
                write!(f, "{}//{}:{} ({})", self.cell, self.package, self.name, cfg)
            }
            None => {
                write!(f, "{}//{}:{}", self.cell, self.package, self.name)
            }
        }
    }
}

impl<'de> Deserialize<'de> for Label {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let label = String::deserialize(deserializer)?;
        Label::new(&label).map_err(D::Error::custom)
    }
}

impl Serialize for Label {
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
    fn parse_label() {
        assert_eq!(
            Ok(Label {
                cell: "abc".into(),
                package: "path/to/target".into(),
                name: "label".into(),
                config: None,
            }),
            Label::new("abc//path/to/target:label".into()),
        );
        assert_eq!(
            Ok(Label {
                cell: "abc".into(),
                package: "path/to/target".into(),
                name: "label".into(),
                config: Some(Box::new(Label {
                    cell: "config".into(),
                    package: "path/to".into(),
                    name: "config".into(),
                    config: None
                })),
            }),
            Label::new("abc//path/to/target:label (config//path/to:config)"),
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
    fn serde() {
        let label: Label =
            serde_json::from_str(r#""abc//path/to/target:label""#).expect("well formed");
        assert_eq!(
            Label {
                cell: "abc".into(),
                package: "path/to/target".into(),
                name: "label".into(),
                config: None,
            },
            label
        );
        assert_eq!(
            r#""abc//path/to/target:label""#,
            serde_json::to_string(&label).expect("infallible")
        );
    }
}
