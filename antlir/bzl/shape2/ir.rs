/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

//! This module defines the data that comprises an Intermediate Representation
//! for shape types. This is designed to be agnostic to how shapes are actually
//! defined (currently with macros in shape.bzl) and just defines the schema of
//! shape types.
//! The IR is used to generate code for Python, Rust, and eventually possible
//! even .bzl files.
//! # Example
//! ## shape.bzl
//! ```
//! shape.shape(
//!   __typename__ = "Greet",
//!   greeting = shape.enum("hello", "good-day", default = "hello"),
//!   to = shape.shape(
//!     __typename__ = "Person",
//!     first_name = str,
//!     last_name = shape.field(str, optional=True),
//!   )
//! )
//! ```
//! ## IR
//! ```
//! Module {
//!   name: "example_shape",
//!   target: "fbcode//antlir:example_shape",
//!   types: {
//!     "Greet": ComplexType::Struct {
//!       fields: {
//!         name: "greeting",
//!         type: ComplexType::Enum {
//!           options: ["hello", "good-day"],
//!         }
//!     ...
//! }
//! ```
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::sync::Arc;

use derive_more::Deref;
use derive_more::Display;
use derive_more::From;
use serde::Deserialize;
use serde::Serialize;

macro_rules! newtype {
    ($x:item) => {
        #[derive(
            Debug,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize,
            From,
            Display,
            Deref
        )]
        #[from(forward)]
        #[display(fmt = "{}", self.0)]
        #[deref(forward)]
        #[serde(transparent)]
        $x
    };
}

// The name of a field, variable or something like that.
newtype!(
    pub struct FieldName(pub String);
);

// The name of a struct, union, enum or other type
newtype!(
    pub struct TypeName(pub String);
);

// A docstring that should never be used for any logic but only for
// adding comments/context to the output
newtype!(
    pub struct DocString(pub String);
);

// A buck target that uniquely identifies a shape module and can be used to
// derive implementation targets or names.
#[derive(
    Debug,
    Clone,
    Serialize,
    Deserialize,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Deref,
    Display
)]
#[deref(forward)]
#[display(fmt = "{}", self.0)]
#[serde(try_from = "String", into = "String")]
#[repr(transparent)]
pub struct Target(String);

impl From<Target> for String {
    fn from(t: Target) -> Self {
        t.to_string()
    }
}

impl TryFrom<String> for Target {
    type Error = anyhow::Error;

    fn try_from(s: String) -> anyhow::Result<Self> {
        // check that the string matches the requirements we have, but don't
        // make unnecessary copies of the string
        anyhow::ensure!(s.contains(':'), "target must contain exactly one ':'");
        anyhow::ensure!(s.ends_with(".shape"), "shape target must end with '.shape'");

        Ok(Self(s))
    }
}

impl TryFrom<&str> for Target {
    type Error = anyhow::Error;

    fn try_from(s: &str) -> anyhow::Result<Self> {
        s.to_owned().try_into()
    }
}

impl Target {
    /// Basename portion of the target
    pub fn basename(&self) -> &str {
        self.0
            .rsplit_once(':')
            .expect("already validated")
            .1
            .strip_suffix(".shape")
            .expect("already validated")
    }

    /// Cell-relative portion of the target
    pub fn base_target(&self) -> &str {
        self.0
            .find("//")
            .map_or(self.0.as_str(), |idx| &self.0[idx..])
            .strip_suffix(".shape")
            .expect("already validated")
    }
}

/// A container to hold all the types defined in a thrift module, as well as
/// pointers to modules that types may be imported from.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Module {
    pub name: String,
    pub target: Target,
    pub types: BTreeMap<TypeName, Arc<Type>>,
    pub docstring: Option<DocString>,
}

impl Module {
    pub fn get_type(&self, name: &TypeName) -> Option<Arc<Type>> {
        self.types.get(name).cloned()
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Type {
    Primitive(Primitive),
    List {
        item_type: Arc<Type>,
    },
    Map {
        key_type: Arc<Type>,
        value_type: Arc<Type>,
    },
    Complex(ComplexType),
    Foreign {
        target: Target,
        name: TypeName,
    },
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ComplexType {
    Enum(Enum),
    Struct(Struct),
    Union(Union),
}

impl ComplexType {
    pub fn name(&self) -> Option<&TypeName> {
        match self {
            Self::Enum(x) => x.name.as_ref(),
            Self::Struct(x) => x.name.as_ref(),
            Self::Union(x) => x.name.as_ref(),
        }
    }

    pub fn set_name(&mut self, name: TypeName) {
        match self {
            Self::Enum(x) => x.name = Some(name),
            Self::Struct(x) => x.name = Some(name),
            Self::Union(x) => x.name = Some(name),
        }
    }
}

newtype!(
    pub struct EnumConstant(String);
);

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Enum {
    pub name: Option<TypeName>,
    pub options: BTreeSet<EnumConstant>,
    pub docstring: Option<DocString>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Struct {
    pub name: Option<TypeName>,
    pub fields: BTreeMap<FieldName, Field>,
    pub docstring: Option<DocString>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Union {
    pub name: Option<TypeName>,
    pub types: Vec<Arc<Type>>,
    pub docstring: Option<DocString>,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
pub struct Field {
    #[serde(rename = "type")]
    pub ty: Arc<Type>,
    pub default_value: Option<serde_json::Value>,
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Primitive {
    Bool,
    I32,
    String,
    Path,
}

#[cfg(test)]
mod tests {
    use anyhow::Result;

    use super::*;

    #[test]
    fn target() -> Result<()> {
        assert_eq!(
            "shape target must end with '.shape'",
            Target::try_from("//some/target:path")
                .unwrap_err()
                .to_string()
        );

        let t: Target = "//some/target:path.shape".try_into()?;
        assert_eq!("path", t.basename());
        assert_eq!("//some/target:path", t.base_target());

        let t: Target = "cell//some/target:path.shape".try_into()?;
        assert_eq!("path", t.basename());
        assert_eq!("//some/target:path", t.base_target());

        let t: Target = ":relative.shape".try_into()?;
        assert_eq!("relative", t.basename());
        assert_eq!(":relative", t.base_target());

        Ok(())
    }
}
