/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Ordering;
use std::cmp::PartialOrd;
use std::hash::Hash;
use std::hash::Hasher;
use std::ops::Deref;
use std::ops::Range;
use std::sync::LazyLock;

use regex::Regex;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::de::Error as _;
use serde::ser::Serializer;
use thiserror::Error;

static ALLOWED_NAME_CHARSET: &str = r"[a-zA-Z0-9,.=\-/~@!+$_#]";
static LABEL_PATTERN: LazyLock<String> = LazyLock::new(|| {
    format!(
        r"(.+?)//({ALLOWED_NAME_CHARSET}*?):({ALLOWED_NAME_CHARSET}*(?:\[{ALLOWED_NAME_CHARSET}+\])?)",
    )
});
static LABEL_WITH_CONFIG_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^{}(?:\s+\((.*)\))?$", *LABEL_PATTERN,)).expect("I know this works")
});
static PACKAGE_RE: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"^(.+?)//({ALLOWED_NAME_CHARSET}*)$")).expect("known good")
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

/// A buck package. Points to a directory (the cell + package path) without any
/// target name, e.g. `cell//path/to/package`. Always fully qualified (with cell
/// name).
#[derive(Clone, Eq)]
pub struct Package {
    full: String,
    cell: Range<usize>,
    path: Range<usize>,
}

impl Package {
    pub fn new(s: impl Into<String>) -> Result<Self, Error> {
        let full: String = s.into();
        match PACKAGE_RE.captures(&full) {
            Some(cap) => {
                let cell = cap.get(1).expect("cell must exist").range();
                let path = cap.get(2).expect("path must exist").range();
                Ok(Self { full, cell, path })
            }
            None => Err(Error::NoMatch(full, PACKAGE_RE.to_string())),
        }
    }

    pub fn cell(&self) -> &str {
        &self.full[self.cell.clone()]
    }

    pub fn path(&self) -> &str {
        &self.full[self.path.clone()]
    }
}

impl PartialEq<Package> for Package {
    fn eq(&self, other: &Package) -> bool {
        self.cell() == other.cell() && self.path() == other.path()
    }
}

impl PartialOrd<Package> for Package {
    fn partial_cmp(&self, other: &Package) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Package {
    fn cmp(&self, other: &Self) -> Ordering {
        (self.cell(), self.path()).cmp(&(other.cell(), other.path()))
    }
}

impl Hash for Package {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.cell().hash(state);
        self.path().hash(state);
    }
}

impl std::str::FromStr for Package {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Self::new(s.to_owned())
    }
}

impl std::fmt::Debug for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        f.debug_tuple("Package").field(&self.to_string()).finish()
    }
}

impl std::fmt::Display for Package {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}//{}", self.cell(), self.path())
    }
}

impl<'de> Deserialize<'de> for Package {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Package::new(s).map_err(D::Error::custom)
    }
}

impl Serialize for Package {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.full)
    }
}

impl AsRef<str> for Package {
    fn as_ref(&self) -> &str {
        &self.full
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;
    use static_assertions::assert_impl_all;

    use super::*;

    assert_impl_all!(Label: Send, Sync);
    assert_impl_all!(Package: Send, Sync);

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

    #[test]
    fn parse_package() {
        let pkg = Package::new("abc//path/to/package").expect("valid package");
        assert_eq!("abc", pkg.cell());
        assert_eq!("path/to/package", pkg.path());
    }

    #[test]
    fn parse_root_package() {
        let pkg = Package::new("abc//").expect("valid root package");
        assert_eq!("abc", pkg.cell());
        assert_eq!("", pkg.path());
    }

    #[rstest]
    #[case::no_cell("//path/to/package")]
    #[case::no_slashes("abc/path/to/package")]
    #[case::has_target("abc//path/to/package:target")]
    fn bad_packages(#[case] s: &str) {
        assert!(Package::new(s).is_err(), "'{}' should not have parsed", s);
    }

    /// The Display impl should reproduce a well-formed package string.
    #[rstest]
    #[case::nested("abc//path/to/package")]
    #[case::root("abc//")]
    fn package_display(#[case] s: &str) {
        let pkg = Package::new(s).expect("well-formed");
        assert_eq!(s, pkg.to_string());
    }

    #[test]
    fn package_serde() {
        let pkg: Package = serde_json::from_str(r#""abc//path/to/package""#).expect("well formed");
        assert_eq!("abc", pkg.cell());
        assert_eq!("path/to/package", pkg.path());
        assert_eq!(
            r#""abc//path/to/package""#,
            serde_json::to_string(&pkg).expect("infallible")
        );
    }
}
