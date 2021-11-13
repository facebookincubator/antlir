/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{anyhow, Context, Result};
use handlebars::{no_escape, Handlebars};
use itertools::{join, Itertools};
use serde::Serialize;
use serde_json::Value as JsonValue;
use std::collections::HashMap;
use std::convert::{TryFrom, TryInto};

use crate::ir::{
    AllTypes, ComplexType, DocString, Enum, Field, FieldName, Primitive, Struct, Type, TypeName,
    Union,
};

static INDENT: &str = "    ";

pub trait Render {
    fn setup_handlebars(hb: Handlebars<'static>) -> Result<Handlebars<'static>>;

    fn render(&self, hb: &Handlebars<'static>) -> Result<String>;
}

pub trait RenderLiteral {
    fn render_literal(&self, value: JsonValue) -> Result<String>;
}

pub trait RenderChecker {
    fn render_typecheck(&self, name: &str, context_name: &str, indent: usize) -> Result<String>;
}

pub(crate) fn render(types: &AllTypes) -> Result<String> {
    let hb = setup_handlebars().context("When setting up handlebars")?;
    let code = types.render(&hb)?;
    let code = code.replace("@_generated", concat!("@", "generated"));
    Ok(code)
}

fn setup_handlebars() -> Result<Handlebars<'static>> {
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    handlebars.register_escape_fn(no_escape);

    let handlebars = AllTypes::setup_handlebars(handlebars)
        .context("When setting up handlebars for AllTypes")?;
    let handlebars = ComplexType::setup_handlebars(handlebars)
        .context("When setting up handlebars for ComplexType")?;
    let handlebars =
        Enum::setup_handlebars(handlebars).context("When setting up handlebars for Enum")?;
    let handlebars =
        Struct::setup_handlebars(handlebars).context("When setting up handlebars for Struct")?;
    let handlebars =
        Union::setup_handlebars(handlebars).context("When setting up handlebars for Union")?;
    Ok(handlebars)
}

static PREAMBLE_TYPES: &[&str] = &["bool", "int", "string", "dict", "list"];

impl Render for AllTypes {
    fn setup_handlebars(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string(
            "preamble",
            include_str!("templates/preamble.bzl.handlebars"),
        )
        .context("Trying to register preamble template")?;
        Ok(hb)
    }

    fn render(&self, hb: &Handlebars<'static>) -> Result<String> {
        let mut output = String::new();

        output.push_str(
            &hb.render("preamble", &PREAMBLE_TYPES)
                .context("Failed to render the preamble")?,
        );

        output.push('\n');

        for (name, typ) in self.into_iter().sorted_by_key(|(t, _)| &t.0) {
            output.push_str(
                &*typ
                    .render(hb)
                    .context(format!("Attempting to render {}", name.0))?,
            );
            output.push('\n');
        }
        Ok(output)
    }
}

impl Render for ComplexType {
    fn setup_handlebars(hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        Ok(hb)
    }

    fn render(&self, hb: &Handlebars<'static>) -> Result<String> {
        match self {
            Self::Enum(e) => e.render(hb),
            Self::Struct(s) => s.render(hb),
            Self::Union(u) => u.render(hb),
        }
    }
}

impl Render for Enum {
    fn setup_handlebars(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string("enum", include_str!("templates/enum.bzl.handlebars"))
            .context("Trying to register enum template")?;
        Ok(hb)
    }

    fn render(&self, hb: &Handlebars<'static>) -> Result<String> {
        hb.render("enum", self)
            .context(format!("When rendering template for {}", self.name.0))
    }
}

impl RenderLiteral for Enum {
    fn render_literal(&self, value: JsonValue) -> Result<String> {
        let value_str = match &value {
            JsonValue::String(s) => serde_starlark::to_string(&value).context(format!(
                "When trying to render string ('{}') for enum {}",
                s, self.name.0
            ))?,
            JsonValue::Number(n) => serde_starlark::to_string(&value).context(format!(
                "When trying to render number ({}) for enum {}",
                n, self.name.0
            ))?,
            other => {
                return Err(anyhow!(
                    "Expected either String or Number when rendering literal for enum '{}' found: {}",
                    self.name.0,
                    value_type_name(other)
                ));
            }
        };

        Ok(format!("{}({})", self.name.0, value_str))
    }
}

impl RenderChecker for Field {
    fn render_typecheck(&self, name: &str, context_name: &str, indent: usize) -> Result<String> {
        if self.required {
            Ok(format!(
                "{indent}if {name} == None:\n\
                {next_indent}_fail_with_context(\"{name}: required but is None\", context={context_name})\n\
                {indent}else:\n\
                {field_validate}\
                ",
                indent = INDENT.repeat(indent),
                next_indent = INDENT.repeat(indent + 1),
                context_name = context_name,
                name = name,
                field_validate = self.typ.render_typecheck(name, context_name, indent + 1)?,
            ))
        } else {
            Ok(format!(
                "{indent}if {name} != None:\n\
                {field_validate}\
                ",
                indent = INDENT.repeat(indent),
                name = name,
                field_validate = self.typ.render_typecheck(name, context_name, indent + 1)?,
            ))
        }
    }
}

#[derive(Debug, Serialize)]
pub struct FieldContext {
    name: FieldName,
    // An already rendered default value. This is expected to be something
    // that could be stored in a dictionary key like: {"foo": {{default_value}}}
    default_value: Option<String>,

    // An already rendered call to check the type. This should be a valid line
    // of code that can be placed inside a function. A variable with the same name
    // as the field you are validating will be in the scope for you to use as well as
    // any of the check_<type> functions defined in the preemable template. You can
    // also assume that globals like fail, None etc are available but beyond this you
    // should not assume anything else about the environment this code runs in (eg
    // don't depend on any other field names or types).
    type_check: String,
    required: bool,
}

impl FieldContext {
    fn try_from_field(f: &Field, indent: usize) -> Result<Self> {
        Ok(Self {
            name: f.name.clone(),
            required: f.required,
            // It is safe to ignore the fact that this might be a union
            // which isn't allowed a default value here because when we go
            // to render that union it will error out there which is a much
            // better error location than trying to do it here.
            default_value: match &f.default_value {
                Some(default_value) => Some(
                    f.typ
                        .render_literal(default_value.clone())
                        .context("While trying to render default value")?,
                ),
                None => None,
            },
            type_check: f
                .render_typecheck(&f.name.0, "err_context", indent)
                .context(format!(
                    "While trying to render typecheck for field {}",
                    f.name.0
                ))?,
        })
    }
}

#[derive(Debug, Serialize)]
pub struct StructOrUnionContext {
    name: TypeName,
    fields: Vec<FieldContext>,
    docstring: Option<DocString>,
}

impl TryFrom<&Struct> for StructOrUnionContext {
    type Error = anyhow::Error;

    fn try_from(s: &Struct) -> Result<Self> {
        let mut fields = Vec::new();
        for field in s.fields.values().sorted_by_key(|f| &f.name.0) {
            fields.push(FieldContext::try_from_field(field, 1).context(format!(
                "When converting {} to renderable format",
                field.name.0
            ))?);
        }

        Ok(Self {
            name: s.name.clone(),
            fields,
            docstring: s.docstring.clone(),
        })
    }
}

impl TryFrom<&Union> for StructOrUnionContext {
    type Error = anyhow::Error;

    fn try_from(u: &Union) -> Result<Self> {
        let mut fields = Vec::new();
        for field in u.fields.values().sorted_by_key(|f| &f.name.0) {
            if field.default_value.is_some() {
                return Err(anyhow!(
                    "'{}.{}' has a default value which isn't allowed for union",
                    u.name.0,
                    field.name.0,
                ));
            }
            fields.push(FieldContext::try_from_field(field, 2).context(format!(
                "When converting {} to renderable format",
                field.name.0
            ))?);
        }

        Ok(Self {
            name: u.name.clone(),
            fields,
            docstring: u.docstring.clone(),
        })
    }
}

impl Render for Struct {
    fn setup_handlebars(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string("struct", include_str!("templates/struct.bzl.handlebars"))
            .context("Trying to register struct template")?;
        Ok(hb)
    }

    fn render(&self, hb: &Handlebars<'static>) -> Result<String> {
        let ctx: StructOrUnionContext = self
            .try_into()
            .context("When trying to build context for struct template")?;

        hb.render("struct", &ctx)
            .context(format!("When rendering template for {}", self.name.0))
    }
}

impl Render for Union {
    fn setup_handlebars(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string("union", include_str!("templates/union.bzl.handlebars"))
            .context("Trying to register union template")?;
        Ok(hb)
    }

    fn render(&self, hb: &Handlebars<'static>) -> Result<String> {
        let ctx: StructOrUnionContext = self
            .try_into()
            .context("When trying to build context for union template")?;

        hb.render("union", &ctx)
            .context(format!("When rendering template for {}", self.name.0))
    }
}

impl RenderLiteral for Type {
    fn render_literal(&self, value: JsonValue) -> Result<String> {
        match self {
            Self::Primitive(p) => p.render_literal(value),
            Self::Complex(c) => c.render_literal(value),
            Self::List { inner_type } => match value {
                JsonValue::Array(values) => {
                    let mut out = Vec::new();
                    for (i, value) in values.into_iter().enumerate() {
                        out.push(
                            inner_type
                                .render_literal(value)
                                .context(format!("While rendering element {} of list", i))?,
                        );
                    }
                    Ok(format!("[{}]", join(out, ",")))
                }
                other => Err(anyhow!(
                    "Expected List but found: {}",
                    value_type_name(&other)
                )),
            },
            Self::Map {
                key_type,
                value_type,
            } => match value {
                JsonValue::Object(object) => {
                    let mut out = Vec::new();
                    for (key, value) in object.into_iter() {
                        let key: JsonValue = serde_json::from_str(&key)
                            .context("Failed to convert key from string back to serde value")?;
                        let key_str = key_type
                            .render_literal(key.clone())
                            .context(format!("While rendering key {}", key))?;

                        let value_str = value_type
                            .render_literal(value)
                            .context(format!("While rendering value for key {}", key))?;

                        out.push(format!("{}: {}", key_str, value_str));
                    }
                    Ok(format!("{{{}}}", join(out, ", ")))
                }
                other => Err(anyhow!(
                    "Expected Map/Object but found: {}",
                    value_type_name(&other)
                )),
            },
        }
    }
}

impl RenderChecker for Type {
    fn render_typecheck(&self, name: &str, context_name: &str, indent: usize) -> Result<String> {
        match self {
            Self::Primitive(p) => p.render_typecheck(name, context_name, indent),
            Self::Complex(c) => c.render_typecheck(name, context_name, indent),
            Self::List { inner_type } => Ok(format!(
                "{indent}_check_list({name}, context={context_name})\n\
                {indent}for (i, {name}_item) in enumerate({name}):\n\
                {next_indent}inner_context = {context_name} + [\"Checking index {{}} for field '{name}'\".format(i)]\n\
                {field_validate}\
                ",
                indent = INDENT.repeat(indent),
                next_indent = INDENT.repeat(indent + 1),
                name = name,
                context_name = context_name,
                field_validate = inner_type
                    .render_typecheck(&format!("{}_item", name), "inner_context", indent + 1)
                    .context("When generating type checker for List inner_type",)?,
            )),
            Self::Map {
                key_type,
                value_type,
            } => Ok(format!(
                "{indent}_check_dict({name}, context={context_name})\n\
                {indent}for {name}_key, {name}_value in {name}.items():\n\
                {next_indent}inner_context = {context_name} + [\"Checking key '{{}}' for field '{name}'\".format({name}_key)]\n\
                {key_validate}\n\
                {value_validate}\
                ",
                indent = INDENT.repeat(indent),
                next_indent = INDENT.repeat(indent + 1),
                name = name,
                context_name = context_name,
                key_validate = key_type
                    .render_typecheck(&format!("{}_key", name), "inner_context", indent + 1)
                    .context("When generating type checker for Map key_type")?,
                value_validate = value_type
                    .render_typecheck(&format!("{}_value", name), "inner_context", indent + 1)
                    .context("When generating type checker for Map value_type")?,
            )),
        }
    }
}

impl RenderLiteral for ComplexType {
    fn render_literal(&self, value: JsonValue) -> Result<String> {
        match self {
            Self::Enum(e) => e.render_literal(value),
            Self::Struct(s) => s.render_literal(value),
            Self::Union(u) => u.render_literal(value),
        }
    }
}
impl RenderChecker for ComplexType {
    fn render_typecheck(&self, name: &str, context_name: &str, indent: usize) -> Result<String> {
        let type_name = match self {
            Self::Enum(e) => &e.name,
            Self::Struct(s) => &s.name,
            Self::Union(u) => &u.name,
        };

        Ok(format!(
            "{}__typecheck_{}({}, err_context={})",
            INDENT.repeat(indent),
            type_name.0,
            name,
            context_name,
        ))
    }
}

impl RenderLiteral for Struct {
    fn render_literal(&self, value: JsonValue) -> Result<String> {
        let mut obj = match value {
            JsonValue::Object(obj) => {
                let mut out = HashMap::new();
                for (key, value) in obj.into_iter() {
                    let key: JsonValue = serde_json::from_str(&key)
                        .context("Failed to convert key from string back to serde value")?;
                    let key_str = match key {
                        JsonValue::String(s) => s,
                        other => {
                            return Err(anyhow!(
                                "Expected field name for struct '{}' to be a String but found: {}",
                                self.name.0,
                                value_type_name(&other)
                            ));
                        }
                    };
                    out.insert(key_str, value);
                }
                out
            }
            other => {
                return Err(anyhow!(
                    "Expected Map/Object when rendering default for {} but found: {}",
                    self.name.0,
                    value_type_name(&other)
                ));
            }
        };

        let mut fields = Vec::new();
        for (field_name, field) in self.fields.iter() {
            let value = match obj.remove(&field_name.0) {
                Some(value) => value,
                None => {
                    if field.required {
                        return Err(anyhow!(
                            "Required field `{}.{}` was not provided in default value. Remaining options: {}",
                            self.name.0,
                            field_name.0,
                            join(obj.keys(), ", "),
                        ));
                    } else {
                        continue;
                    }
                }
            };

            let value_str = field.typ.render_literal(value).context(format!(
                "While rendering `{}.{}`",
                self.name.0, field_name.0
            ))?;

            fields.push(format!("{}={}", field_name.0, value_str));
        }

        if !obj.is_empty() {
            return Err(anyhow!(
                "Default value in json contains fields that don't exist on {}: {}",
                self.name.0,
                join(obj.keys(), ", "),
            ));
        }

        Ok(format!("{}({})", self.name.0, join(fields, ", ")))
    }
}

impl RenderLiteral for Union {
    fn render_literal(&self, value: JsonValue) -> Result<String> {
        let obj = match value {
            JsonValue::Object(obj) => obj,
            other => {
                return Err(anyhow!(
                    "Expected Map/Object when rendering default for '{}' but found: {}",
                    self.name.0,
                    value_type_name(&other)
                ));
            }
        };

        if obj.len() != 1 {
            return Err(anyhow!(
                "Expected exactly 1 value for union '{}' found {} ({})",
                self.name.0,
                obj.len(),
                join(obj.keys(), ", "),
            ));
        }

        let (key, value) = obj
            .into_iter()
            .next()
            .expect("Found no item after checking len == 1");

        let key = FieldName(key);
        let field = match self.fields.get(&key) {
            Some(f) => f,
            None => {
                return Err(anyhow!(
                    "Default value provided field name '{}' which wasn't found on this Union ('{}')",
                    key.0,
                    self.name.0,
                ));
            }
        };

        let value_str = field.typ.render_literal(value).context(format!(
            "While trying to render value for '{}.{}'",
            self.name.0, field.name.0
        ))?;

        Ok(format!("{}({}={})", self.name.0, key.0, value_str))
    }
}

impl RenderLiteral for Primitive {
    fn render_literal(&self, value: JsonValue) -> Result<String> {
        Ok(match (self, &value) {
            // Bools in thrift are given to us as 1 and 0 so this will
            // be wrong if we use the serde_starlark conversion
            (Self::Bool, JsonValue::Number(n)) => {
                serde_starlark::to_string(&(n != &serde_json::Number::from(0)))
            }
            (_, _) => serde_starlark::to_string(&value),
        }?)
    }
}

impl RenderChecker for Primitive {
    fn render_typecheck(&self, name: &str, context_str: &str, indent: usize) -> Result<String> {
        let add_context_str = format!("add_context(\"Validating '{}'\", {})", name, context_str);
        match self {
            Self::Bool => Ok(format!(
                "{}_check_bool({}, context={})",
                INDENT.repeat(indent),
                name,
                add_context_str
            )),
            Self::Byte | Self::I16 | Self::I32 | Self::I64 => Ok(format!(
                "{}_check_int({}, context={})",
                INDENT.repeat(indent),
                name,
                add_context_str
            )),
            Self::String => Ok(format!(
                "{}_check_string({}, context={})",
                INDENT.repeat(indent),
                name,
                add_context_str
            )),
            _ => Err(anyhow!("No starlark type checker available for {:?}", self)),
        }
    }
}

fn value_type_name(value: &JsonValue) -> &'static str {
    match value {
        JsonValue::Null => "Null",
        JsonValue::Bool(..) => "Bool",
        JsonValue::Number(..) => "Number",
        JsonValue::String(..) => "String",
        JsonValue::Array(..) => "Array",
        JsonValue::Object(..) => "Object",
    }
}
