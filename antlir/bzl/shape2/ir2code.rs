/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

use std::fs::File;
use std::ops::Deref;
use std::path::PathBuf;

use anyhow::{anyhow, Context as _, Result};
use derive_more::{AsRef, Deref, From};
use handlebars::{handlebars_helper, no_escape, Handlebars};
use itertools::Itertools;
use serde::Serialize;
use serde_json::Value;
use structopt::{clap::arg_enum, StructOpt};

use ir::{ComplexType, Enum, Field, Module, Primitive, Struct, Target, Type, TypeName, Union};

arg_enum! {
    #[derive(Debug)]
    enum RenderFormat {
        // classic style shapes with very limited type safety
        Pydantic,
        // plain rust structs with serde
        Rust,
    }
}

#[derive(StructOpt)]
struct Opts {
    format: RenderFormat,
    // path to json-serialized IR
    ir: PathBuf,
}

pub fn main() -> Result<()> {
    let opts = Opts::from_args();
    let f =
        File::open(&opts.ir).with_context(|| format!("failed to open {}", opts.ir.display()))?;
    let ir: Module = serde_json::from_reader(f)
        .with_context(|| format!("failed to deserialize {}", opts.ir.display()))?;
    let code = match opts.format {
        RenderFormat::Pydantic => render::<Pydantic>(&ir),
        RenderFormat::Rust => render::<Rust>(&ir),
    }
    .context("failed to render code")?;
    println!("{}", code);
    Ok(())
}

#[derive(Debug, AsRef, Deref, Clone, PartialEq, Eq, From)]
#[deref(forward)]
#[from(forward)]
#[as_ref(forward)]
#[repr(transparent)]
pub struct Pydantic(String);

#[derive(Debug, AsRef, Deref, Clone, PartialEq, Eq, From)]
#[deref(forward)]
#[from(forward)]
#[as_ref(forward)]
#[repr(transparent)]
pub struct Rust(String);
pub trait RegisterTemplates<T> {
    fn register_templates(hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        Ok(hb)
    }
}

pub trait Render<T>: RegisterTemplates<T> + Serialize + Sized
where
    T: From<String>,
{
    const ENTRYPOINT: &'static str;

    fn render(&self, hb: &Handlebars<'static>) -> Result<T> {
        hb.render(Self::ENTRYPOINT, self)
            .with_context(|| format!("while rendering template '{}'", Self::ENTRYPOINT))
            .map(T::from)
    }
}

pub trait ToLiteral<T>
where
    T: From<String>,
{
    fn to_literal(&self) -> T;
}

pub(crate) fn render<T>(types: &Module) -> Result<String>
where
    Module: Render<T>,
    ComplexType: Render<T>,
    Enum: Render<T>,
    Struct: Render<T>,
    Union: Render<T>,
    T: Deref<Target = str>,
    T: From<String>,
{
    let hb = setup_handlebars::<T>().context("When setting up handlebars")?;
    let code: T = types.render(&hb)?;
    let code = code.replace("@_generated", concat!('@', "generated"));
    Ok(code)
}

fn setup_handlebars<T>() -> Result<Handlebars<'static>>
where
    Module: Render<T>,
    ComplexType: Render<T>,
    Enum: Render<T>,
    Struct: Render<T>,
    Union: Render<T>,
    T: From<String>,
{
    let mut handlebars = Handlebars::new();
    handlebars.set_strict_mode(true);
    handlebars.register_escape_fn(no_escape);
    handlebars.register_helper("has-default-value", Box::new(has_default_value));
    handlebars.register_helper("json", Box::new(json_helper));
    handlebars.register_helper("upper", Box::new(upper_helper));
    handlebars.register_helper("ident", Box::new(ident_helper));

    let handlebars = <Module as RegisterTemplates<T>>::register_templates(handlebars)
        .context("When setting up handlebars for Module")?;
    let handlebars = <ComplexType as RegisterTemplates<T>>::register_templates(handlebars)
        .context("When setting up handlebars for ComplexType")?;
    let handlebars =
        Enum::register_templates(handlebars).context("When setting up handlebars for Enum")?;
    let handlebars =
        Struct::register_templates(handlebars).context("When setting up handlebars for Struct")?;
    let handlebars =
        Union::register_templates(handlebars).context("When setting up handlebars for Union")?;

    Ok(handlebars)
}

handlebars_helper!(upper_helper: |x: String| x.to_uppercase());
handlebars_helper!(ident_helper: |x: String| x.replace('-', "_"));

trait ModuleExt<T>
where
    ComplexType: Render<T>,
    T: Deref<Target = str>,
    T: From<String>,
{
    fn render_types(&self, hb: &Handlebars<'static>) -> Result<T>;
}

impl<T> ModuleExt<T> for Module
where
    ComplexType: Render<T>,
    T: Deref<Target = str>,
    T: From<String>,
{
    fn render_types(&self, hb: &Handlebars<'static>) -> Result<T> {
        let mut output = String::new();
        // For python, types have to be ordered correctly to avoid forward
        // declarations. Rust does not have these same requirements, but it's
        // simpler to just always sort consistently. Simply order types by the
        // number of types they transitively depend on, and forward references
        // will be eliminated (the IR cannot contain cycles, which makes this
        // much simpler than a full topological sort).
        for (name, typ) in self
            .types
            .iter()
            .sorted_by_key(|(_, typ)| typ.transitive_dependency_count())
        {
            let rendered: T = match typ.deref() {
                Type::Complex(ct) => ct.render(hb),
                _ => Err(anyhow!("unsupported top-level type {:?}", typ)),
            }
            .with_context(|| format!("while attempting to render type {}", name.0))?;
            output.push_str(&rendered);
            output.push('\n');
        }
        Ok(T::from(output))
    }
}

impl RegisterTemplates<Pydantic> for Module {
    fn register_templates(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string(
            "module",
            include_str!("templates/module.pydantic.handlebars"),
        )
        .context("Trying to register preamble template")?;
        hb.register_helper("type-hint", Box::new(py_type_hint));
        hb.register_helper("literal", Box::new(py_literal));
        Ok(hb)
    }
}

trait TargetExt {
    fn rust_crate(&self) -> String;
    fn python_module(&self) -> String;
}

impl TargetExt for Target {
    fn rust_crate(&self) -> String {
        format!("::{}", self.basename())
    }

    fn python_module(&self) -> String {
        self.base_target()
            .replace("/", ".")
            .replace(":", ".")
            .trim_start_matches('.')
            .to_owned()
    }
}

trait TypeExt {
    fn py_type_hint(&self) -> TypeName;
    fn rs_type(&self) -> TypeName;
    fn rs_union_name(&self) -> TypeName;
    fn transitive_dependency_count(&self) -> usize;
}

impl TypeExt for Type {
    fn py_type_hint(&self) -> TypeName {
        match self {
            Self::Primitive(p) => TypeName(
                match p {
                    Primitive::Bool => "bool",
                    Primitive::I32 => "int",
                    Primitive::String => "str",
                    Primitive::Path => "Path",
                }
                .to_string(),
            ),
            Self::List { item_type } => {
                // lie and say that lists are tuples to discourage mutation
                TypeName(format!("typing.Tuple[{}, ...]", item_type.py_type_hint()))
            }
            Self::Map {
                key_type,
                value_type,
            } => TypeName(format!(
                "typing.Mapping[{}, {}]",
                key_type.py_type_hint(),
                value_type.py_type_hint(),
            )),
            Self::Complex(c) => c
                .name()
                .expect("cannot codegen shape without top-level name")
                .clone(),
            Self::Foreign { target, name } => TypeName(format!(
                "importlib.import_module(\"{}\").{}",
                target.python_module(),
                name
            )),
        }
    }

    fn rs_type(&self) -> TypeName {
        match self {
            Self::Primitive(p) => TypeName(
                match p {
                    Primitive::Bool => "bool",
                    Primitive::I32 => "i32",
                    Primitive::String => "String",
                    Primitive::Path => "::shape::ShapePath",
                }
                .to_string(),
            ),
            Self::List { item_type } => TypeName(format!("Vec<{}>", item_type.rs_type())),
            Self::Map {
                key_type,
                value_type,
            } => TypeName(format!(
                "BTreeMap<{}, {}>",
                key_type.rs_type(),
                value_type.rs_type(),
            )),
            Self::Complex(c) => TypeName(format!(
                "crate::{}",
                c.name()
                    .expect("cannot codegen shape without top-level name")
                    .clone()
            )),
            Self::Foreign { target, name } => {
                TypeName(format!("{}::{}", target.rust_crate(), name))
            }
        }
    }

    fn rs_union_name(&self) -> TypeName {
        match self {
            Self::Primitive(p) => TypeName(
                match p {
                    Primitive::Bool => "Bool",
                    Primitive::I32 => "I32",
                    Primitive::String => "String",
                    Primitive::Path => "Path",
                }
                .to_string(),
            ),
            Self::List { item_type } => TypeName(format!("ListOf_{}", item_type.rs_union_name())),
            Self::Map {
                key_type,
                value_type,
            } => TypeName(format!(
                "Dict_{}_To_{}",
                key_type.rs_union_name(),
                value_type.rs_union_name(),
            )),
            Self::Complex(c) => c
                .name()
                .expect("cannot codegen shape without top-level name")
                .clone(),
            Self::Foreign { target, name } => {
                TypeName(format!("{}::{}", target.rust_crate(), name))
            }
        }
    }

    /// Count how many other types this type (transitively) depends on,
    /// including itself.
    fn transitive_dependency_count(&self) -> usize {
        match self {
            Self::Primitive(_) => 0,
            Self::List { item_type } => 1 + item_type.transitive_dependency_count(),
            Self::Map {
                key_type,
                value_type,
            } => {
                2 + key_type.transitive_dependency_count()
                    + value_type.transitive_dependency_count()
            }
            Self::Complex(complex) => {
                1 + match complex {
                    ComplexType::Struct(s) => s
                        .fields
                        .values()
                        .map(|f| f.ty.transitive_dependency_count())
                        .sum(),
                    ComplexType::Union(u) => u
                        .types
                        .iter()
                        .map(|ty| ty.transitive_dependency_count())
                        .sum(),
                    ComplexType::Enum(_) => 0,
                }
            }
            Self::Foreign { target: _, name: _ } => 1,
        }
    }
}

handlebars_helper!(has_default_value: |field: Field| Value::Bool(field.default_value.is_some()));
handlebars_helper!(py_type_hint: |ty: Type| ty.py_type_hint().to_string());

impl ToLiteral<Pydantic> for Value {
    fn to_literal(&self) -> Pydantic {
        match self {
            Value::Null => "None".to_string(),
            Value::Bool(b) => match b {
                true => "True",
                false => "False",
            }
            .to_string(),
            Value::Number(n) => n.to_string(),
            Value::String(s) => format!(r#""{}""#, s),
            Value::Array(a) => format!(
                "[{}]",
                a.iter()
                    .map(|v| ToLiteral::<Pydantic>::to_literal(v).0)
                    .join(",")
            ),
            Value::Object(o) => format!(
                "{{{}}}",
                o.iter()
                    .map(|(k, v)| (k, ToLiteral::<Pydantic>::to_literal(v).0))
                    .map(|(k, v)| format!(r#""{}": {}"#, k, v))
                    .join(",")
            ),
        }
        .into()
    }
}

handlebars_helper!(py_literal: |v: Value| v.to_literal().to_string());

impl Render<Pydantic> for Module {
    const ENTRYPOINT: &'static str = "module";

    fn render(&self, hb: &Handlebars<'static>) -> Result<Pydantic> {
        let mut output = String::new();

        output.push_str(
            &hb.render("module", &())
                .context("failed to render the module preamble")?,
        );

        output.push('\n');

        let types: Pydantic = self.render_types(hb)?;
        output.push_str(&types);
        Ok(Pydantic(output))
    }
}

impl RegisterTemplates<Rust> for Module {
    fn register_templates(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string("lib", include_str!("templates/lib.rs.handlebars"))
            .context("Trying to register preamble template")?;
        hb.register_helper("type", Box::new(rs_type));
        hb.register_helper("union-name", Box::new(rs_union_name));
        Ok(hb)
    }
}

handlebars_helper!(rs_type: |ty: Type| ty.rs_type().to_string());
handlebars_helper!(rs_union_name: |ty: Type| ty.rs_union_name().to_string());
handlebars_helper!(json_helper: |v: Value| v.to_string());

impl Render<Rust> for Module {
    const ENTRYPOINT: &'static str = "lib";

    fn render(&self, hb: &Handlebars<'static>) -> Result<Rust> {
        let mut output = String::new();

        output.push_str(
            &hb.render("lib", &())
                .context("failed to render the module preamble")?,
        );

        output.push('\n');

        let types: Rust = self.render_types(hb)?;
        output.push_str(&types);
        Ok(Rust(output))
    }
}

impl<T> RegisterTemplates<T> for ComplexType {}

impl<T> Render<T> for ComplexType
where
    Enum: Render<T>,
    Struct: Render<T>,
    Union: Render<T>,
    T: From<String>,
{
    const ENTRYPOINT: &'static str = "<passthrough>";

    fn render(&self, hb: &Handlebars<'static>) -> Result<T> {
        match self {
            Self::Enum(e) => e.render(hb),
            Self::Struct(s) => s.render(hb),
            Self::Union(u) => u.render(hb),
        }
    }
}

impl RegisterTemplates<Pydantic> for Enum {
    fn register_templates(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string("enum", include_str!("templates/enum.pydantic.handlebars"))
            .context("Trying to register enum template")?;
        Ok(hb)
    }
}

impl RegisterTemplates<Rust> for Enum {
    fn register_templates(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string("enum", include_str!("templates/enum.rs.handlebars"))
            .context("Trying to register enum template")?;
        Ok(hb)
    }
}

impl<T> Render<T> for Enum
where
    Enum: RegisterTemplates<T>,
    T: From<String>,
{
    const ENTRYPOINT: &'static str = "enum";
}

impl RegisterTemplates<Pydantic> for Struct {
    fn register_templates(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string(
            "struct",
            include_str!("templates/struct.pydantic.handlebars"),
        )
        .context("Trying to register struct template")?;
        Ok(hb)
    }
}

impl RegisterTemplates<Rust> for Struct {
    fn register_templates(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string("struct", include_str!("templates/struct.rs.handlebars"))
            .context("Trying to register struct template")?;
        Ok(hb)
    }
}

impl<T> Render<T> for Struct
where
    Struct: RegisterTemplates<T>,
    T: From<String>,
{
    const ENTRYPOINT: &'static str = "struct";
}

impl RegisterTemplates<Pydantic> for Union {
    fn register_templates(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string("union", include_str!("templates/union.pydantic.handlebars"))
            .context("Trying to register union template")?;
        Ok(hb)
    }
}

impl RegisterTemplates<Rust> for Union {
    fn register_templates(mut hb: Handlebars<'static>) -> Result<Handlebars<'static>> {
        hb.register_template_string("union", include_str!("templates/union.rs.handlebars"))
            .context("Trying to register union template")?;
        Ok(hb)
    }
}

impl<T> Render<T> for Union
where
    Union: RegisterTemplates<T>,
    T: From<String>,
{
    const ENTRYPOINT: &'static str = "union";
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn targetext() -> Result<()> {
        let t: Target = "cell//some/target:path.shape".try_into()?;
        assert_eq!("some.target.path", t.python_module());
        assert_eq!("::path", t.rust_crate());
        Ok(())
    }
}
