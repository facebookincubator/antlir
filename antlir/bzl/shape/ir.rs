/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{anyhow, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::rc::Rc;

// The name of a field, variable or something like that.
#[derive(Debug, Clone, PartialEq, Hash, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct FieldName(pub String);

// The name of a struct, union, enum or other type
#[derive(Debug, Clone, PartialEq, Hash, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct TypeName(pub String);

// A docstring that should never be used for any logic but only for
// adding comments/context to the output
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(transparent)]
pub struct DocString(pub String);

// A container to hold all the top level types
#[derive(Debug)]
pub(crate) struct AllTypes {
    types: HashMap<TypeName, Rc<ComplexType>>,
}

impl<'a> IntoIterator for &'a AllTypes {
    type Item = (&'a TypeName, &'a Rc<ComplexType>);
    type IntoIter = std::collections::hash_map::Iter<'a, TypeName, Rc<ComplexType>>;

    fn into_iter(self) -> Self::IntoIter {
        self.types.iter()
    }
}

impl AllTypes {
    pub fn new() -> Self {
        Self {
            types: HashMap::new(),
        }
    }

    // Add the item to the hashmap and give you back a reference to it so that you
    // can use it to build more types
    pub(crate) fn add(&mut self, name: &TypeName, typ: ComplexType) -> Result<Rc<ComplexType>> {
        if self.types.contains_key(&name) {
            Err(anyhow!("Attempted to add duplicate type {}", name.0))
        } else {
            let rc_type = Rc::new(typ);
            self.types.insert(name.clone(), rc_type.clone());
            Ok(rc_type)
        }
    }

    pub fn get(&self, name: &TypeName) -> Option<&Rc<ComplexType>> {
        self.types.get(name).clone()
    }
}

#[derive(Debug)]
pub(crate) enum Type {
    Primitive(Primitive),
    List {
        inner_type: Box<Type>,
    },
    Map {
        key_type: Box<Type>,
        value_type: Box<Type>,
    },
    Complex(Rc<ComplexType>),
}

#[derive(Debug)]
pub(crate) enum ComplexType {
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
}

#[derive(Debug, Serialize)]
pub(crate) struct EnumConstant {
    pub name: FieldName,
    pub value: i32,
}

#[derive(Debug, Serialize)]
pub(crate) struct Enum {
    pub name: TypeName,
    pub options: HashMap<FieldName, EnumConstant>,
    pub docstring: Option<DocString>,
}

#[derive(Debug)]
pub(crate) struct Struct {
    pub name: TypeName,
    pub fields: HashMap<FieldName, Field>,
    pub docstring: Option<DocString>,
}

#[derive(Debug)]
pub(crate) struct Union {
    pub name: TypeName,
    pub fields: HashMap<FieldName, Field>,
    pub docstring: Option<DocString>,
}

#[derive(Debug)]
pub(crate) struct Field {
    pub name: FieldName,
    pub typ: Type,
    pub default_value: Option<serde_json::Value>,
    pub required: bool,
}

#[derive(Debug)]
pub(crate) enum Primitive {
    Bool,
    Byte,
    I16,
    I32,
    I64,
    Float,
    Double,
    Binary,
    String,
}
