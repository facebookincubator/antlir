/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

/// This module defines the data that comprises an Intermediate Representation
/// for shape types. This is designed to be agnostic to how shapes are actually
/// defined (currently with macros in shape.bzl) and just defines the schema of
/// shape types.
/// The IR is used to generate code for Python, Rust, and eventually possible
/// even .bzl files.
/// # Example
/// ## shape.bzl
/// ```
/// shape.shape(
///   __typename__ = "Greet",
///   greeting = shape.enum("hello", "good-day", default = "hello"),
///   to = shape.shape(
///     __typename__ = "Person",
///     first_name = str,
///     last_name = shape.field(str, optional=True),
///   )
/// )
/// ```
/// ## IR
/// ```
/// Module {
///   name: "example_shape",
///   target: "fbcode//antlir:example_shape",
///   types: {
///     "Greet": ComplexType::Struct {
///       fields: {
///         name: "greeting",
///         type: ComplexType::Enum {
///           options: ["hello", "good-day"],
///         }
///     ...
/// }
/// ```
use derive_more::{Deref, Display, From, FromStr};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::rc::Rc;

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
        #[display(forward)]
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
newtype!(
    #[derive(FromStr)]
    pub struct Target(pub String);
);

/// A container to hold all the types defined in a thrift module, as well as
/// pointers to modules that types may be imported from.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub name: String,
    pub target: Target,
    #[serde(default)]
    pub imports: Vec<Module>,
    pub types: BTreeMap<TypeName, Rc<Type>>,
}

impl Module {
    pub fn get_type(&self, name: &TypeName) -> Option<Rc<Type>> {
        self.types.get(name).cloned()
    }
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Type {
    Primitive(Primitive),
    List {
        item_type: Rc<Type>,
    },
    // TODO: this is not easily definable in thrift, which may pose a problem in
    // the future
    Tuple {
        item_types: Vec<Rc<Type>>,
    },
    Map {
        key_type: Rc<Type>,
        value_type: Rc<Type>,
    },
    Complex(ComplexType),
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ComplexType {
    Enum(Enum),
    Struct(Struct),
    Union(Union),
}

impl ComplexType {
    pub fn name(&self) -> &TypeName {
        match self {
            Self::Enum(e) => &e.name,
            Self::Struct(s) => &s.name,
            Self::Union(u) => &u.name,
        }
    }

    pub fn target(&self) -> &Target {
        match self {
            Self::Enum(e) => &e.target,
            Self::Struct(s) => &s.target,
            Self::Union(u) => &u.target,
        }
    }
}

newtype!(
    pub struct EnumConstant(FieldName);
);

#[derive(Debug, Deserialize, Serialize)]
pub struct Enum {
    pub name: TypeName,
    pub options: BTreeMap<FieldName, EnumConstant>,
    pub docstring: Option<DocString>,
    pub target: Target,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Struct {
    pub name: TypeName,
    pub fields: BTreeMap<FieldName, Field>,
    pub docstring: Option<DocString>,
    pub target: Target,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Union {
    pub name: TypeName,
    pub types: Vec<Rc<Type>>,
    pub docstring: Option<DocString>,
    pub target: Target,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct Field {
    pub name: FieldName,
    #[serde(rename = "type")]
    pub typ: Rc<Type>,
    pub default_value: Option<serde_json::Value>,
    pub required: bool,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Primitive {
    Bool,
    Byte,
    I16,
    I32,
    I64,
    Float,
    Double,
    Binary,
    String,
    Path,
}
