/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(get_mut_unchecked)]
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Arc;

use allocative::Allocative;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use derive_more::Deref;
use derive_more::Display;
use json_arg::JsonFile;
use serde::Deserialize;
use slotmap::SlotMap;
use starlark::any::ProvidesStaticType;
use starlark::environment::FrozenModule;
use starlark::environment::GlobalsBuilder;
use starlark::environment::LibraryExtension;
use starlark::environment::Module;
use starlark::eval::Evaluator;
use starlark::eval::FileLoader;
use starlark::starlark_module;
use starlark::starlark_simple_value;
use starlark::syntax::AstModule;
use starlark::syntax::Dialect;
use starlark::values::dict::UnpackDictEntries;
use starlark::values::function::NativeFunction;
use starlark::values::list_or_tuple::UnpackListOrTuple;
use starlark::values::starlark_value;
use starlark::values::structs::AllocStruct;
use starlark::values::AllocValue;
use starlark::values::NoSerialize;
use starlark::values::StarlarkValue;
use starlark::values::StringValue;
use starlark::values::UnpackValue;
use starlark::values::Value;
use starlark::values::ValueLike;

fn bzl_globals() -> GlobalsBuilder {
    GlobalsBuilder::extended_by(&[
        // TODO(nga): drop extensions which are not needed.
        LibraryExtension::StructType,
        LibraryExtension::RecordType,
        LibraryExtension::EnumType,
        LibraryExtension::Map,
        LibraryExtension::Filter,
        LibraryExtension::Partial,
        LibraryExtension::Debug,
        LibraryExtension::Print,
        LibraryExtension::Pprint,
        LibraryExtension::Breakpoint,
        LibraryExtension::Json,
        LibraryExtension::Typing,
    ])
}

/// We have a bit of an easier job than loading arbitrary bzl files. We know
/// that the target graph is not circular (since buck has already walked it
/// before we ever call this binary) and we can enforce that we are given the
/// full set of bzl dependencies.
fn eval_and_freeze_module(
    deps: &Dependencies,
    ast: AstModule,
) -> Result<(FrozenModule, SlotMap<TypeId, Arc<ir::Type>>)> {
    let module = Module::new();
    let globals = bzl_globals().build();
    let registry = TypeRegistryRefCell::default();
    {
        let mut evaluator: Evaluator = Evaluator::new(&module);
        evaluator.set_loader(deps);
        // Disable the gc so that Value identity is stable. Our modules are
        // very small and we throw away the Starlark internals as soon as we
        // convert to the IR, so this is fine.
        evaluator.disable_gc();
        evaluator.extra = Some(&registry);

        evaluator
            .eval_module(ast, &globals)
            .map_err(starlark::Error::into_anyhow)?;
    }
    Ok((
        module.freeze().context("while freezing module")?,
        registry.0.into_inner().0,
    ))
}

slotmap::new_key_type! {
    /// TypeId and TypeRegistry exist to store unique references to complex types.
    /// These are types that end up getting codegenned, not primitives.
    #[derive(ProvidesStaticType, NoSerialize, Allocative)]
    #[allocative(skip)]
    struct TypeId;
}
impl std::fmt::Display for TypeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}
starlark_simple_value!(TypeId);
#[starlark_value(type = "TypeId")]
impl<'v> StarlarkValue<'v> for TypeId {
    fn invoke(
        &self,
        me: Value<'v>,
        args: &starlark::eval::Arguments<'v, '_>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<Value<'v>> {
        let reg = get_type_registry(eval)?
            .try_borrow()
            .map_err(starlark::Error::new_other)?;
        let ty = me.try_to_type(&reg)?;
        match ty.as_ref() {
            ir::Type::Complex(ir::ComplexType::Struct(_)) => {
                args.no_positional_args(eval.heap())?;
                // no need to type-check, since it will already be done at buck parse
                // time, and will also be done when loading the json
                Ok(eval.heap().alloc(AllocStruct(args.names_map()?)))
            }
            ir::Type::Complex(ir::ComplexType::Enum(_)) => {
                args.no_named_args()?;
                args.positional1(eval.heap())
            }
            _ => Err(starlark::Error::new(starlark::ErrorKind::Other(anyhow!(
                "only structs and enums are callable, not ({ty:#?})"
            )))),
        }
    }
}

#[derive(Debug, ProvidesStaticType, Default, Deref)]
struct TypeRegistryRefCell(RefCell<TypeRegistry>);

#[derive(Debug, ProvidesStaticType, Default)]
struct TypeRegistry(SlotMap<TypeId, Arc<ir::Type>>);

impl TypeRegistry {
    fn add(&mut self, ty: ir::Type) -> TypeId {
        self.0.insert(Arc::new(ty))
    }

    fn get(&self, id: TypeId) -> Option<Arc<ir::Type>> {
        self.0.get(id).cloned()
    }
}

#[derive(Debug, Clone, Display, ProvidesStaticType, NoSerialize, Allocative)]
#[display(fmt = "{:?}", self)]
#[repr(transparent)]
#[allocative(skip)]
struct StarlarkType(Arc<ir::Type>);
starlark_simple_value!(StarlarkType);
#[starlark_value(type = "StarlarkType")]
impl<'v> StarlarkValue<'v> for StarlarkType {}

trait TryToType {
    fn try_to_type(&self, reg: &TypeRegistry) -> Result<Arc<ir::Type>>;
}

impl<'v> TryToType for Value<'v> {
    fn try_to_type(&self, reg: &TypeRegistry) -> Result<Arc<ir::Type>> {
        if let Some(tid) = self.downcast_ref::<TypeId>() {
            reg.get(*tid).ok_or_else(|| anyhow!("{:?} not found", tid))
        } else if let Some(nf) = self.downcast_ref::<NativeFunction>() {
            Ok(Arc::new(match nf.to_string().as_str() {
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

#[derive(Debug, Clone, Display, ProvidesStaticType, NoSerialize, Allocative)]
#[display(fmt = "{:?}", self)]
#[repr(transparent)]
#[allocative(skip)]
struct StarlarkField(ir::Field);
starlark_simple_value!(StarlarkField);
#[starlark_value(type = "StarlarkField")]
impl<'v> StarlarkValue<'v> for StarlarkField {}

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
    fn shape<'v>(
        #[starlark(require = named)] __thrift: Option<UnpackDictEntries<u32, &'v str>>,
        #[starlark(kwargs)] kwargs: UnpackDictEntries<&'v str, Value<'v>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<TypeId> {
        let mut reg = get_type_registry(eval)?.try_borrow_mut()?;
        let fields: BTreeMap<ir::FieldName, Arc<ir::Field>> = kwargs
            .entries
            .into_iter()
            .filter(|(key, _)| (*key != "__I_AM_TARGET__") && (*key != "__thrift_fields"))
            .map(|(key, val)| val.try_to_field(&reg).map(|f| (key.into(), Arc::new(f))))
            .collect::<Result<_>>()?;
        let thrift_fields = __thrift
            .map(|t| {
                t.entries
                    .into_iter()
                    .map(|(k, v)| {
                        let field_name = ir::FieldName(v.to_owned());
                        let field = fields
                            .get(&field_name)
                            .with_context(|| format!("'{}' is not a thrift field", k))?
                            .clone();
                        Ok((k, (field_name, field)))
                    })
                    .collect::<Result<_>>()
            })
            .transpose()
            .context(
                "Thrift fields don't match shape fields. This would already have failed in Buck",
            )?;
        let s = ir::Struct {
            fields,
            thrift_fields,
            docstring: None,
            name: None,
        };
        let ty = ir::Type::Complex(ir::ComplexType::Struct(s));
        Ok(reg.add(ty))
    }

    fn list<'v>(ty: Value<'v>, eval: &mut Evaluator) -> anyhow::Result<StarlarkType> {
        let reg = get_type_registry(eval)?.try_borrow()?;
        ty.try_to_type(&reg)
            .map(|ty| ir::Type::List { item_type: ty })
            .map(Arc::new)
            .map(StarlarkType)
    }

    fn r#enum<'v>(
        #[starlark(args)] args: Value<'v>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> anyhow::Result<TypeId> {
        let mut reg = get_type_registry(eval)?.try_borrow_mut()?;
        let options = args
            .iterate(eval.heap())
            .map_err(starlark::Error::into_anyhow)
            .context("while collecting enum variants")?
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

    fn field<'v>(
        ty: Value<'v>,
        #[starlark(default = false)] optional: bool,
        default: Option<Value<'v>>,
        eval: &mut Evaluator,
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

    fn new<'v>(
        shape: Value<'v>,
        #[starlark(kwargs)] kwargs: UnpackDictEntries<StringValue<'v>, Value<'v>>,
    ) -> anyhow::Result<AllocStruct<Vec<(StringValue<'v>, Value<'v>)>>> {
        let _ = shape;
        // no need to type-check, since it will already be done at buck parse
        // time, and will also be done when loading the json
        Ok(AllocStruct(kwargs.entries))
    }

    fn dict<'v>(
        key_type: Value<'v>,
        value_type: Value<'v>,
        eval: &mut Evaluator,
    ) -> anyhow::Result<StarlarkType> {
        let reg = get_type_registry(eval)?.try_borrow()?;
        let key_type = key_type
            .try_to_type(&reg)
            .context("dict key must be a type")?;
        let value_type = value_type
            .try_to_type(&reg)
            .context("dict value must be a type")?;
        Ok(StarlarkType(Arc::new(ir::Type::Map {
            key_type,
            value_type,
        })))
    }

    fn union<'v>(
        #[starlark(args)] args: Value<'v>,
        #[starlark(require = named)] __thrift: Option<UnpackListOrTuple<u32>>,
        eval: &mut Evaluator<'v, '_, '_>,
    ) -> starlark::Result<TypeId> {
        let mut reg = get_type_registry(eval)?
            .try_borrow_mut()
            .map_err(starlark::Error::new_other)?;
        let types: Vec<_> = args
            .iterate(eval.heap())?
            .enumerate()
            .map(|(i, v)| {
                v.try_to_type(&reg)
                    .with_context(|| format!("union item at {} is not a type", i))
            })
            .collect::<Result<_>>()?;
        let thrift_types = match __thrift {
            Some(t) => {
                // Closure to convert the `anyhow` error to a `starlark::Error`
                (|| {
                    ensure!(
                        t.items.len() == types.len(),
                        "Mismatched number of fields. This would have already failed in Buck."
                    );
                    Ok(())
                })()?;
                Some(
                    t.into_iter()
                        .zip(&types)
                        .map(|(thrift_num, typ)| (thrift_num, typ.clone()))
                        .collect(),
                )
            }
            None => None,
        };
        let u = ir::Union {
            types,
            thrift_types,
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

#[derive(Debug, Clone, Deserialize)]
#[serde(transparent)]
#[repr(transparent)]
struct Dependencies(BTreeMap<ir::Target, PathBuf>);

impl FileLoader for Dependencies {
    fn load(&self, load: &str) -> Result<FrozenModule> {
        // shape.bzl itself is an implicit dependency and comes with a native
        // implementation
        if load == "//antlir/bzl:shape.bzl" || load == "@antlir//antlir/bzl:shape.bzl" {
            let ast = AstModule::parse("", "shape = shape_impl".to_string(), &Dialect::Standard)
                .map_err(starlark::Error::into_anyhow)?;
            let module = Module::new();
            {
                let mut evaluator: Evaluator = Evaluator::new(&module);
                let globals = bzl_globals().with_struct("shape_impl", shape).build();
                evaluator
                    .eval_module(ast, &globals)
                    .map_err(starlark::Error::into_anyhow)?;
            }
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
                    std::fs::File::open(p).with_context(|| format!("while loading {:?}", p))?;
                serde_json::from_reader(&mut f).with_context(|| format!("while parsing {:?}", p))
            })
            .and_then(ir_to_module)
    }
}

fn ir_to_module(m: ir::Module) -> Result<FrozenModule> {
    let module = Module::new();
    for name in m.types.into_keys() {
        let ty = StarlarkType(Arc::new(ir::Type::Foreign {
            target: m.target.clone(),
            name: name.clone(),
        }));
        module.set(&name, ty.alloc_value(module.heap()));
    }
    module.freeze()
}

fn starlark_to_ir(
    f: FrozenModule,
    types: SlotMap<TypeId, Arc<ir::Type>>,
    target: ir::Target,
) -> Result<ir::Module> {
    let named_types: BTreeMap<ir::TypeName, _> = f
        .names()
        // grab the Value that is assigned to this name from the starlark module (this is the TypeId)
        .filter_map(|n| f.get(&n).ok().map(|v| (ir::TypeName::from(n.as_str()), v)))
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
                match Arc::get_mut_unchecked(&mut ty) {
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
            .docs
            .map(|ds| format!("{}\n{}", ds.summary, ds.details.unwrap_or_else(String::new)))
            .map(|s| s.into()),
    })
}

#[derive(Debug, Parser)]
struct Opts {
    #[clap(long)]
    target: ir::Target,
    #[clap(long)]
    entrypoint: PathBuf,
    #[clap(long)]
    deps: JsonFile<Dependencies>,
    #[clap(long)]
    out: PathBuf,
}

fn main() -> Result<()> {
    let opts = Opts::parse();
    let dialect = Dialect {
        enable_load_reexport: false,
        ..Dialect::Extended
    };
    let ast =
        AstModule::parse_file(&opts.entrypoint, &dialect).map_err(starlark::Error::into_anyhow)?;
    let (f, types) = eval_and_freeze_module(&opts.deps, ast)
        .with_context(|| format!("while processing {:?}", opts.entrypoint))?;

    let module =
        starlark_to_ir(f, types, opts.target).context("while converting to high-level IR")?;
    std::fs::write(
        &opts.out,
        serde_json::to_string_pretty(&module).expect("failed to serialize IR"),
    )
    .context("while writing output")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use maplit::btreemap;

    use super::*;

    #[test]
    fn simple_module() -> starlark::Result<()> {
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
                    "top".into() => Arc::new(ir::Type::Complex(ir::ComplexType::Struct(ir::Struct{
                        name: Some("top".into()),
                        fields :btreemap! {
                            "hello".into() => Arc::new(ir::Field {
                                ty: Arc::new(ir::Type::Primitive(ir::Primitive::String)),
                                default_value: None,
                                required: true,
                            })
                        },
                        thrift_fields: None,
                        docstring: None,
                    })))
                },
                docstring: None,
            }
        );
        Ok(())
    }

    #[test]
    fn ignores_irrelevant_top_level() -> starlark::Result<()> {
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
    fn bad_enum_variant_type() -> starlark::Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
shape.enum("a", "b", 3)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let err = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast)
            .expect_err("should have failed");
        assert!(
            err.to_string()
                .contains("Type of parameters mismatch, expected `str`, actual `int`"),
            "{:?}",
            err.to_string()
        );
        Ok(())
    }

    #[test]
    fn bad_dict_key() -> starlark::Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
shape.dict(42, str)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let err = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast)
            .expect_err("should have failed")
            .to_string();
        assert!(err.contains("dict key must be a type"), "{:?}", err);
        Ok(())
    }

    #[test]
    fn bad_dict_val() -> starlark::Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
shape.dict(str, 42)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let err = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast)
            .expect_err("should have failed")
            .to_string();
        assert!(err.contains("dict value must be a type"), "{:?}", err,);
        Ok(())
    }

    #[test]
    fn bad_union_item() -> starlark::Result<()> {
        let ast = AstModule::parse(
            "simple_module",
            r#"load("//antlir/bzl:shape.bzl", "shape")
shape.union(str, 42, int)
"#
            .to_string(),
            &Dialect::Extended,
        )?;
        let err = eval_and_freeze_module(&Dependencies(BTreeMap::new()), ast)
            .expect_err("should have failed")
            .to_string();
        assert!(err.contains("union item at 1 is not a type"), "{:?}", err);
        Ok(())
    }
}
