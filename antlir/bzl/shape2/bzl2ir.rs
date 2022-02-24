/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(get_mut_unchecked)]
use anyhow::{anyhow, bail, Context, Result};
use derive_more::{Deref, Display};
use gazebo::any::AnyLifetime;
use serde::Deserialize;
use slotmap::SlotMap;
use starlark::environment::{FrozenModule, Globals, GlobalsBuilder, Module};
use starlark::eval::{Evaluator, FileLoader};
use starlark::syntax::{AstModule, Dialect};
use starlark::values::dict::DictOf;
use starlark::values::docs::DocItem;
use starlark::values::function::NativeFunction;
use starlark::values::structs::StructGen;
use starlark::values::{AllocValue, StarlarkValue, StringValue, UnpackValue, Value, ValueLike};
use starlark::{starlark_module, starlark_simple_value, starlark_type};
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::rc::Rc;
use structopt::StructOpt;

/// We have a bit of an easier job than loading arbitrary bzl files. We know
/// that the target graph is not circular (since buck has already walked it
/// before we ever call this binary) and we can enforce that we are given the
/// full set of bzl dependencies.
fn eval_and_freeze_module(
    deps: &Dependencies,
    ast: AstModule,
) -> Result<(FrozenModule, SlotMap<TypeId, Rc<ir::Type>>)> {
    let module = Module::new();
    let globals = Globals::extended();
    let mut evaluator: Evaluator = Evaluator::new(&module);
    evaluator.set_loader(deps);
    // Disable the gc so that Value identity is stable. Our modules are
    // very small and we throw away the Starlark internals as soon as we
    // convert to the IR, so this is fine.
    evaluator.disable_gc();
    let registry = TypeRegistryRefCell::default();
    evaluator.extra = Some(&registry);

    evaluator.eval_module(ast, &globals)?;
    Ok((
        module.freeze().context("while freezing module")?,
        registry.0.into_inner().0,
    ))
}

slotmap::new_key_type! {
    /// TypeId and TypeRegistry exist to store unique references to complex types.
    /// These are types that end up getting codegenned, not primitives.
    struct TypeId;
}
impl std::fmt::Display for TypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
starlark_simple_value!(TypeId);
impl<'v> StarlarkValue<'v> for TypeId {
    starlark_type!("TypeId");
}

#[derive(Debug, AnyLifetime, Default, Deref)]
struct TypeRegistryRefCell(RefCell<TypeRegistry>);

#[derive(Debug, AnyLifetime, Default)]
struct TypeRegistry(SlotMap<TypeId, Rc<ir::Type>>);

impl TypeRegistry {
    fn add(&mut self, ty: ir::Type) -> TypeId {
        self.0.insert(Rc::new(ty))
    }

    fn get(&self, id: TypeId) -> Option<Rc<ir::Type>> {
        self.0.get(id).cloned()
    }
}

#[derive(Debug, Clone, Display)]
#[display(fmt = "{:?}", self)]
#[repr(transparent)]
struct StarlarkType(Rc<ir::Type>);
starlark_simple_value!(StarlarkType);
impl<'v> StarlarkValue<'v> for StarlarkType {
    starlark_type!("StarlarkType");
}

trait TryToType {
    fn try_to_type(&self, reg: &TypeRegistry) -> Result<Rc<ir::Type>>;
}

impl<'v> TryToType for Value<'v> {
    fn try_to_type(&self, reg: &TypeRegistry) -> Result<Rc<ir::Type>> {
        if let Some(tid) = self.downcast_ref::<TypeId>() {
            reg.get(*tid).ok_or_else(|| anyhow!("{:?} not found", tid))
        } else if let Some(nf) = self.downcast_ref::<NativeFunction>() {
            Ok(Rc::new(match nf.to_string().as_str() {
                "bool" => ir::Type::Primitive(ir::Primitive::Bool),
                "int" => ir::Type::Primitive(ir::Primitive::I32),
                "str" => ir::Type::Primitive(ir::Primitive::String),
                "path" => ir::Type::Primitive(ir::Primitive::Path),
                _ => bail!("expected a primitive, found a function '{}'", nf),
            }))
        } else if let Some(ty) = self.downcast_ref::<StarlarkType>() {
            Ok(ty.0.clone())
        } else {
            Err(anyhow!("cannot convert {:?} to type", self))
        }
    }
}

trait TryToField {
    fn try_to_field(&self, reg: &TypeRegistry) -> Result<ir::Field>;
}

impl<'v> TryToField for Value<'v> {
    fn try_to_field(&self, reg: &TypeRegistry) -> Result<ir::Field> {
        if let Ok(ty) = self.try_to_type(reg) {
            Ok(ir::Field {
                ty,
                required: true,
                default_value: None,
            })
        } else if let Some(f) = self.downcast_ref::<StarlarkField>() {
            Ok(f.0.clone())
        } else {
            Err(anyhow!("cannot convert {:?} to field", self))
        }
    }
}

#[derive(Debug, Clone, Display)]
#[display(fmt = "{:?}", self)]
#[repr(transparent)]
struct StarlarkField(ir::Field);
starlark_simple_value!(StarlarkField);
impl<'v> StarlarkValue<'v> for StarlarkField {
    starlark_type!("StarlarkField");
}

fn get_type_registry<'a>(eval: &'a Evaluator) -> Result<&'a TypeRegistryRefCell> {
    let extra = eval
        .extra
        .context("extra should be TypeRegistry, but was not present")?;
    extra
        .downcast_ref::<TypeRegistryRefCell>()
        .with_context(|| {
            format!(
                "extra should be TypeRegistry, but was {:?}",
                extra.static_type_of()
            )
        })
}

#[starlark_module]
fn shape(builder: &mut GlobalsBuilder) {
    fn shape(kwargs: DictOf<'v, &str, Value<'v>>) -> anyhow::Result<TypeId> {
        let mut reg = get_type_registry(eval)?.try_borrow_mut()?;
        let fields = kwargs
            .to_dict()
            .into_iter()
            .filter(|(key, _)| *key != "__I_AM_TARGET__")
            .map(|(key, val)| val.try_to_field(&reg).map(|f| (key.into(), f)))
            .collect::<Result<_>>()?;
        let s = ir::Struct {
            fields,
            docstring: None,
            name: None,
        };
        let ty = ir::Type::Complex(ir::ComplexType::Struct(s));
        Ok(reg.add(ty))
    }

    fn list(ty: Value<'v>) -> anyhow::Result<StarlarkType> {
        let reg = get_type_registry(eval)?.try_borrow()?;
        ty.try_to_type(&reg)
            .map(|ty| ir::Type::List { item_type: ty })
            .map(Rc::new)
            .map(StarlarkType)
    }

    fn r#enum(args: Value<'v>) -> anyhow::Result<TypeId> {
        let mut reg = get_type_registry(eval)?.try_borrow_mut()?;
        let options = args
            .iterate_collect(heap)
            .context("while collecting enum variants")?
            .into_iter()
            .map(|v| String::unpack_param(v).map(|s| s.into()))
            .collect::<Result<_>>()?;
        let enm = ir::Enum {
            options,
            docstring: None,
            name: None,
        };
        let ty = ir::Type::Complex(ir::ComplexType::Enum(enm));
        Ok(reg.add(ty))
    }

    fn field(
        ty: Value<'v>,
        optional @ false: bool,
        default: Option<Value<'v>>,
    ) -> anyhow::Result<StarlarkField> {
        let reg = get_type_registry(eval)?.try_borrow()?;
        ty.try_to_type(&reg)
            .and_then(|ty| {
                Ok(ir::Field {
                    ty,
                    required: !optional,
                    default_value: default
                        .map(|d| {
                            d.to_json().and_then(|s| {
                                serde_json::from_str(&s).context("parsing result of to_json")
                            })
                        })
                        .transpose()?,
                })
            })
            .map(StarlarkField)
    }

    fn new(
        _shape: Value<'v>,
        kwargs: DictOf<'v, StringValue<'v>, Value<'v>>,
    ) -> anyhow::Result<StructGen<'v, Value<'v>>> {
        // no need to type-check, since it will already be done at buck parse
        // time, and will also be done when loading the json
        Ok(StructGen::new(kwargs.to_dict()))
    }

    fn dict(key_type: Value<'v>, value_type: Value<'v>) -> anyhow::Result<StarlarkType> {
        let reg = get_type_registry(eval)?.try_borrow()?;
        let key_type = key_type
            .try_to_type(&reg)
            .context("dict key must be a type")?;
        let value_type = value_type
            .try_to_type(&reg)
            .context("dict value must be a type")?;
        Ok(StarlarkType(Rc::new(ir::Type::Map {
            key_type,
            value_type,
        })))
    }

    fn tuple(args: Value<'v>) -> anyhow::Result<StarlarkType> {
        let reg = get_type_registry(eval)?.try_borrow()?;
        let item_types = args
            .iterate_collect(heap)?
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                v.try_to_type(&reg)
                    .with_context(|| format!("tuple item at {} is not a type", i))
            })
            .collect::<Result<_>>()?;
        Ok(StarlarkType(Rc::new(ir::Type::Tuple { item_types })))
    }

    fn union(args: Value<'v>) -> anyhow::Result<TypeId> {
        let mut reg = get_type_registry(eval)?.try_borrow_mut()?;
        let types = args
            .iterate_collect(heap)?
            .into_iter()
            .enumerate()
            .map(|(i, v)| {
                v.try_to_type(&reg)
                    .with_context(|| format!("union item at {} is not a type", i))
            })
            .collect::<Result<_>>()?;
        let u = ir::Union {
            types,
            docstring: None,
            name: None,
        };
        let ty = ir::Type::Complex(ir::ComplexType::Union(u));
        Ok(reg.add(ty))
    }

    fn path() -> anyhow::Result<StarlarkField> {
        Err(anyhow!("shape.path is no longer a callable function"))
    }
}

#[derive(Deserialize)]
#[serde(transparent)]
#[repr(transparent)]
struct Dependencies(BTreeMap<ir::Target, PathBuf>);

impl FileLoader for Dependencies {
    fn load(&self, load: &str) -> Result<FrozenModule> {
        // shape.bzl itself is an implicit dependency and comes with a native
        // implementation
        if load == "//antlir/bzl:shape.bzl" {
            let ast = AstModule::parse("", "shape = shape_impl".to_string(), &Dialect::Standard)?;
            let module = Module::new();
            let mut evaluator: Evaluator = Evaluator::new(&module);
            let globals = GlobalsBuilder::extended()
                .with_struct("shape_impl", shape)
                .build();
            evaluator.eval_module(ast, &globals)?;
            return module.freeze();
        }
        let load: ir::Target = load
            .strip_suffix(".bzl")
            .unwrap_or(load)
            .try_into()
            .with_context(|| format!("while parsing '{}'", load))?;
        self.0
            .get(&load)
            .with_context(|| {
                format!(
                    "'{}' is not explicitly listed in `deps`, refusing to load",
                    load
                )
            })
            .and_then(|p| {
                let mut f =
                    std::fs::File::open(&p).with_context(|| format!("while loading {:?}", p))?;
                serde_json::from_reader(&mut f).with_context(|| format!("while parsing {:?}", p))
            })
            .and_then(ir_to_module)
    }
}

fn ir_to_module(m: ir::Module) -> Result<FrozenModule> {
    let module = Module::new();
    for name in m.types.into_keys() {
        let ty = StarlarkType(Rc::new(ir::Type::Foreign {
            target: m.target.clone(),
            name: name.clone(),
        }));
        module.set(&name, ty.alloc_value(module.heap()));
    }
    module.freeze()
}

fn starlark_to_ir(
    f: FrozenModule,
    types: SlotMap<TypeId, Rc<ir::Type>>,
    target: ir::Target,
) -> Result<ir::Module> {
    let named_types: BTreeMap<ir::TypeName, _> = f
        .names()
        // grab the Value that is assigned to this name from the starlark module (this is the TypeId)
        .filter_map(|n| f.get(n).map(|v| (ir::TypeName::from(n), v)))
        // only TypeIds matter, any other top-level variables can be safely
        // ignored for now, since they are not (directly) relevant to generated
        // code
        .filter_map(|(name, v)| {
            v.downcast::<TypeId>()
                .ok()
                .map(|tid| (name, tid.as_ref().clone()))
        })
        .map(|(name, tid)| {
            let mut ty = types
                .get(tid)
                .with_context(|| format!("{:?} was not found in the registry", tid))?
                .clone();
            // Yeah, this is technically unsafe, but all we're doing is setting the
            // name, and I need to set the name in all borrows of this type, at any
            // level of nesting. This can be removed if/when the IR is smart enough
            // to store references to types declared in the same module,
            // similarly to how foreign types work.
            unsafe {
                match Rc::get_mut_unchecked(&mut ty) {
                    ir::Type::Complex(ct) => ct.set_name(name.clone()),
                    _ => unreachable!("all top-level types are ComplexType"),
                };
            };
            Ok((name, ty))
        })
        .collect::<Result<_>>()?;

    Ok(ir::Module {
        name: target.basename().to_string(),
        target,
        types: named_types,
        docstring: f
            .documentation()
            .and_then(|d| match d {
                DocItem::Module(m) => m.docs,
                _ => unreachable!(),
            })
            .map(|ds| format!("{}\n{}", ds.summary, ds.details.unwrap_or_else(String::new)))
            .map(|s| s.into()),
    })
}

#[derive(StructOpt)]
struct Opts {
    #[structopt(parse(try_from_str = ir::Target::try_from))]
    target: ir::Target,
    entrypoint: PathBuf,
    #[structopt(parse(try_from_str = serde_json::from_str))]
    deps: Dependencies,
}

fn main() -> Result<()> {
    let opts = Opts::from_args();
    let dialect = Dialect {
        enable_load_reexport: false,
        ..Dialect::Extended
    };
    let ast = AstModule::parse_file(&opts.entrypoint, &dialect)?;
    let (f, types) = eval_and_freeze_module(&opts.deps, ast)
        .with_context(|| format!("while processing {:?}", opts.entrypoint))?;

    let module =
        starlark_to_ir(f, types, opts.target).context("while converting to high-level IR")?;
    println!("{}", serde_json::to_string_pretty(&module).unwrap());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::btreemap;

    #[test]
    fn simple_module() -> Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
top = shape.shape(hello=str)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let (f, types) = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast)?;
        let m = starlark_to_ir(f, types, "//antlir/shape:simple.shape".try_into()?)?;
        assert_eq!(
            m,
            ir::Module {
                name: "simple".into(),
                target: "//antlir/shape:simple.shape".try_into()?,
                types: btreemap! {
                    "top".into() => Rc::new(ir::Type::Complex(ir::ComplexType::Struct(ir::Struct{
                        name: Some("top".into()),
                        fields :btreemap! {
                            "hello".into() => ir::Field {
                                ty: Rc::new(ir::Type::Primitive(ir::Primitive::String)),
                                default_value: None,
                                required: true,
                            }
                        },
                        docstring: None,
                    })))
                },
                docstring: None,
            }
        );
        Ok(())
    }

    #[test]
    fn ignores_irrelevant_top_level() -> Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
irrelevant = "hello world"

ty = shape.shape(hello=str)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let (_, types) = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast)?;
        assert_eq!(1, types.len());
        Ok(())
    }

    #[test]
    fn bad_enum_variant_type() -> Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
shape.enum("a", "b", 3)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let err = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast).unwrap_err();
        assert!(
            err.to_string()
                .contains("Type of parameters mismatch, expected `str`, actual `int`"),
            "{:?}",
            err.to_string()
        );
        Ok(())
    }

    #[test]
    fn bad_dict_key() -> Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
shape.dict(42, str)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let err = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast)
            .unwrap_err()
            .to_string();
        assert!(err.contains("dict key must be a type"), "{:?}", err);
        Ok(())
    }

    #[test]
    fn bad_dict_val() -> Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
shape.dict(str, 42)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let err = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast)
            .unwrap_err()
            .to_string();
        assert!(err.contains("dict value must be a type"), "{:?}", err,);
        Ok(())
    }

    #[test]
    fn bad_tuple_item() -> Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
shape.tuple(str, 42, int)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let err = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast)
            .unwrap_err()
            .to_string();
        assert!(err.contains("tuple item at 1 is not a type"), "{:?}", err);
        Ok(())
    }

    #[test]
    fn bad_union_item() -> Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
shape.union(str, 42, int)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let err = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast)
            .unwrap_err()
            .to_string();
        assert!(err.contains("union item at 1 is not a type"), "{:?}", err);
        Ok(())
    }
}
