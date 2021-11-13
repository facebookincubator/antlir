/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{anyhow, Context, Result};
use itertools::join;
use serde::Deserialize;
use std::collections::HashMap;
use std::convert::TryFrom;
use std::rc::Rc;

use crate::ir::{
    AllTypes, ComplexType, DocString, Enum, EnumConstant, Field, FieldName, Primitive, Struct,
    Type, TypeName, Union,
};

#[derive(Debug, Deserialize)]
pub struct ParsedTop {
    pub(crate) namespaces: Option<HashMap<String, String>>,
    pub(crate) structs: Option<HashMap<TypeName, ParsedStruct>>,
    pub(crate) enumerations: Option<HashMap<TypeName, ParsedEnum>>,
}

impl ParsedTop {
    pub fn from_reader<R: std::io::Read>(reader: R) -> Result<Self, serde_json::Error> {
        serde_json::from_reader(reader)
    }

    // Adds a given target_type to the provided all_types structure.
    // Also provide a list of seen TypeNames so that you can detect loops
    fn add_enum_to_alltypes(
        &self,
        target_type: &TypeName,
        all_types: &mut AllTypes,
    ) -> Result<Rc<ComplexType>> {
        if let Some(ct) = all_types.get(target_type) {
            match **ct {
                ComplexType::Enum(_) => {}
                ComplexType::Struct(_) | ComplexType::Union(_) => {
                    return Err(anyhow!(
                        "Thrift json corrupt: '{}' is listed as both an enum and a struct!",
                        target_type.0
                    ));
                }
            }
            return Ok(ct.clone());
        }

        let e = match &self.enumerations {
            Some(enums) => match enums.get(target_type) {
                Some(e) => e,
                None => {
                    return Err(anyhow!(
                        "Enum '{}' was requested but not found",
                        target_type.0
                    ));
                }
            },
            None => {
                return Err(anyhow!(
                    "Enum '{}' was requested but self.enumerations is empty",
                    target_type.0
                ));
            }
        };
        if &e.name != target_type {
            return Err(anyhow!(
                "Thrift json corrupt: Fetched '{}' from enum map but found '{}'",
                target_type.0,
                e.name.0,
            ));
        }

        Ok(all_types
            .add(target_type, ComplexType::Enum(e.clone().into()))
            .context(format!("When adding enum '{}'", target_type.0))?)
    }

    // Adds a given target_type to the provided all_types structure.
    // Also provide a list of seen TypeNames so that you can detect loops
    fn add_struct_to_alltypes(
        &self,
        target_type: &TypeName,
        all_types: &mut AllTypes,
        mut seen: Vec<TypeName>,
    ) -> Result<Rc<ComplexType>> {
        if let Some(ct) = all_types.get(target_type) {
            match **ct {
                ComplexType::Struct(_) | ComplexType::Union(_) => {}
                ComplexType::Enum(_) => {
                    return Err(anyhow!(
                        "Thrift json corrupt: '{}' is listed as both an enum and a struct!",
                        target_type.0
                    ));
                }
            }
            return Ok(ct.clone());
        }

        if seen.contains(target_type) {
            return Err(anyhow!(
                "Circular reference found between types: {} -> {}",
                join(seen.into_iter().map(|s| s.0), " -> "),
                target_type.0,
            ));
        }
        seen.push(target_type.clone());

        let s = match &self.structs {
            Some(structs) => match structs.get(target_type) {
                Some(s) => s,
                None => {
                    return Err(anyhow!(
                        "Struct '{}' was requested but not found",
                        target_type.0
                    ));
                }
            },
            None => {
                return Err(anyhow!(
                    "Struct '{}' was requested but self.structs is empty",
                    target_type.0
                ));
            }
        };
        if &s.name != target_type {
            return Err(anyhow!(
                "Thrift json corrupt: Fetched '{}' from map but found '{}'",
                target_type.0,
                s.name.0,
            ));
        }
        if s.is_exception {
            return Err(anyhow!(
                "'{}' is an exception which is unsupported",
                s.name.0
            ));
        }

        let mut fields = HashMap::new();
        for (name, field) in s.fields.iter() {
            if name != &field.name {
                return Err(anyhow!(
                    "Thrift json corrupt: Field on struct '{}' is called '{}' in map but '{}' inside",
                    s.name.0,
                    name.0,
                    field.name.0,
                ));
            }
            let field_type: Type = self
                .parsed_type_to_type(&field.typ, all_types, seen.clone())
                .context(format!("When processing '{}.{}'", s.name.0, name.0))?;

            fields.insert(
                name.clone(),
                Field {
                    name: name.clone(),
                    typ: field_type,
                    default_value: field.default_value.clone(),
                    required: (&field.required).into(),
                },
            );
        }

        let new_type = if s.is_union {
            ComplexType::Union(Union {
                name: s.name.clone(),
                fields,
                docstring: s.docstring.clone(),
            })
        } else {
            ComplexType::Struct(Struct {
                name: s.name.clone(),
                fields,
                docstring: s.docstring.clone(),
            })
        };
        Ok(all_types
            .add(target_type, new_type)
            .context(format!("When adding struct '{}'", target_type.0))?)
    }

    fn parsed_type_to_type(
        &self,
        typ: &ParsedType,
        all_types: &mut AllTypes,
        seen: Vec<TypeName>,
    ) -> Result<Type> {
        Ok(match typ {
            ParsedType::Primitive(p) => Type::Primitive(p.into()),
            ParsedType::Complex(c) => match c {
                ParsedComplexType::List { inner_type } => {
                    let list_type = self
                        .parsed_type_to_type(&*inner_type, all_types, seen.clone())
                        .context(format!("When adding list type"))?;

                    Type::List {
                        inner_type: Box::new(list_type),
                    }
                }
                ParsedComplexType::Map {
                    key_type,
                    value_type,
                } => {
                    let key_type = self
                        .parsed_type_to_type(&*key_type, all_types, seen.clone())
                        .context(format!("When adding key type for map"))?;

                    let value_type = self
                        .parsed_type_to_type(&*value_type, all_types, seen.clone())
                        .context(format!("When adding value type for map"))?;

                    Type::Map {
                        key_type: Box::new(key_type),
                        value_type: Box::new(value_type),
                    }
                }
                ParsedComplexType::Struct { name } => {
                    Type::Complex(self.add_struct_to_alltypes(name, all_types, seen.clone())?)
                }
                ParsedComplexType::Enum { name } => {
                    Type::Complex(self.add_enum_to_alltypes(name, all_types)?)
                }
            },
        })
    }
}

impl<'a> TryFrom<ParsedTop> for AllTypes {
    type Error = anyhow::Error;

    fn try_from(top: ParsedTop) -> Result<AllTypes> {
        let mut all_types = Self::new();
        if let Some(structs) = &top.structs {
            for name in structs.keys() {
                top.add_struct_to_alltypes(name, &mut all_types, Vec::new())
                    .context(format!("When trying to add struct '{}'", name.0))?;
            }
        }
        if let Some(enums) = &top.enumerations {
            for name in enums.keys() {
                top.add_enum_to_alltypes(name, &mut all_types)
                    .context(format!("When trying to add enum '{}'", name.0))?;
            }
        }

        Ok(all_types)
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ParsedStruct {
    pub name: TypeName,
    pub fields: HashMap<FieldName, ParsedField>,
    pub docstring: Option<DocString>,
    pub is_union: bool,
    pub is_exception: bool,
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ParsedEnum {
    pub name: TypeName,
    pub constants: HashMap<FieldName, ParsedEnumConstant>,
    pub docstring: Option<DocString>,
}

impl From<ParsedEnum> for Enum {
    fn from(e: ParsedEnum) -> Self {
        Self {
            name: e.name,
            options: e
                .constants
                .into_iter()
                .map(|(n, c)| (n, c.into()))
                .collect(),
            docstring: e.docstring,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ParsedEnumConstant {
    pub name: FieldName,
    pub value: i32,
}

impl From<ParsedEnumConstant> for EnumConstant {
    fn from(ec: ParsedEnumConstant) -> Self {
        Self {
            name: ec.name,
            value: ec.value,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum ParsedRequiredness {
    Required,
    OptInReqOut,
    Optional,
}

impl From<&ParsedRequiredness> for bool {
    fn from(parsed: &ParsedRequiredness) -> bool {
        match parsed {
            ParsedRequiredness::Required | ParsedRequiredness::OptInReqOut => true,
            ParsedRequiredness::Optional => false,
        }
    }
}

#[derive(Debug, Deserialize)]
pub(crate) struct ParsedField {
    pub name: FieldName,
    #[serde(rename = "type")]
    pub typ: ParsedType,
    pub default_value: Option<serde_json::Value>,
    pub required: ParsedRequiredness,
}

#[derive(Debug, Deserialize)]
#[serde(untagged)]
pub(crate) enum ParsedType {
    Primitive(ParsedPrimitive),
    Complex(ParsedComplexType),
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ParsedPrimitive {
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

impl From<&ParsedPrimitive> for Primitive {
    fn from(parsed: &ParsedPrimitive) -> Self {
        match parsed {
            ParsedPrimitive::Bool => Self::Bool,
            ParsedPrimitive::Byte => Self::Byte,
            ParsedPrimitive::I16 => Self::I16,
            ParsedPrimitive::I32 => Self::I32,
            ParsedPrimitive::I64 => Self::I64,
            ParsedPrimitive::Float => Self::Float,
            ParsedPrimitive::Double => Self::Double,
            ParsedPrimitive::Binary => Self::Binary,
            ParsedPrimitive::String => Self::String,
        }
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "lowercase", tag = "type")]
pub(crate) enum ParsedComplexType {
    List {
        inner_type: Box<ParsedType>,
    },
    Map {
        key_type: Box<ParsedType>,
        value_type: Box<ParsedType>,
    },
    Struct {
        name: TypeName,
    },
    Enum {
        name: TypeName,
    },
}
