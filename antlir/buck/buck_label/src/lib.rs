/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::de::Deserializer;
use serde::de::Error as _;
use serde::ser::Serializer;
use serde::Deserialize;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error, Copy, Clone, PartialEq, Eq)]
pub enum Error {
    #[error("label does not contain a cell")]
    MissingCell,
    #[error("label must contain exactly one ':'")]
    TargetSeparator,
}

/// A buck target label. Points to a specific target and is always fully
/// qualified (aka, with cell name).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Label {
    cell: String,
    package: String,
    name: String,
}

impl Label {
    pub fn new(s: String) -> Result<Self, Error> {
        if !s.contains("//") {
            return Err(Error::MissingCell);
        }
        if s.chars().filter(|s| *s == ':').count() != 1 {
            return Err(Error::TargetSeparator);
        }
        match s.split_once("//") {
            Some((cell, rest)) => {
                if cell.is_empty() {
                    Err(Error::MissingCell)
                } else {
                    match rest.split_once(':') {
                        Some((package, name)) => Ok(Self {
                            cell: cell.into(),
                            package: package.into(),
                            name: name.into(),
                        }),
                        None => Err(Error::TargetSeparator),
                    }
                }
            }
            None => Err(Error::MissingCell),
        }
    }

    /// Escape the Label to be used in a filename. This flattens the label space
    /// so that a directory hierarchy does not need to be created to match the
    /// repo structure (in other words, '/' gets replaced).
    pub fn flat_filename(&self) -> String {
        format!(
            "{}@{}:{}",
            self.cell,
            self.package.replace('/', "_"),
            self.name
        )
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
}

impl std::str::FromStr for Label {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        Self::new(s.into())
    }
}

impl std::fmt::Display for Label {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}//{}:{}", self.cell, self.package, self.name)
    }
}

impl<'de> Deserialize<'de> for Label {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let label = String::deserialize(deserializer)?;
        Label::new(label).map_err(D::Error::custom)
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
            }),
            Label::new("abc//path/to/target:label".into()),
        );
        assert_eq!(
            Err(Error::MissingCell),
            Label::new("//path/to/target:label".into()),
        );
        assert_eq!(
            Err(Error::TargetSeparator),
            Label::new("abc//path/to/target/label".into()),
        );
        assert_eq!(
            Err(Error::TargetSeparator),
            Label::new("abc//path/to/target::label".into()),
        );
    }

    #[test]
    fn escape() {
        let label: Label = Label::new("abc//path/to/target:label".into()).expect("well-formed");
        assert_eq!("abc@path_to_target:label", label.flat_filename());
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
            },
            label
        );
        assert_eq!(
            r#""abc//path/to/target:label""#,
            serde_json::to_string(&label).expect("infallible")
        );
    }
}
