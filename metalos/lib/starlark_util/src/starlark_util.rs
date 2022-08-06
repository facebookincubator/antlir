/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::marker::PhantomData;

use anyhow::anyhow;
use anyhow::Result;
use gazebo::any::ProvidesStaticType;
use serde::Serialize;
use starlark::starlark_type;
use starlark::values::AllocFrozenValue;
use starlark::values::AllocValue;
use starlark::values::FrozenHeap;
use starlark::values::FrozenValue;
use starlark::values::Heap;
use starlark::values::StarlarkValue;
use starlark::values::Value;

mod value;
use value::Value as ThriftValue;

/// Expose a Thrift struct to Starlark. All the fields on this struct will be
/// exposed to any Starlark code, including recursive structs or other
/// collections.
#[derive(Debug, Serialize)]
pub struct Struct<T>(value::Struct, PhantomData<T>);

impl<T> Struct<T> {
    pub fn new(t: &T) -> Result<Self>
    where
        T: fbthrift::Serialize<value::Protocol> + std::panic::RefUnwindSafe + Debug,
    {
        let v = value::to_value(t)?;
        match v {
            ThriftValue::Struct(s) => Ok(Self(s, PhantomData)),
            // Unfortunately, generated thrift code doesn't allow us to make
            // this check statically. It's something that we could easily add,
            // but is outside the scope of this feature.
            _ => Err(anyhow!("{:?} is not a thrift struct", t)),
        }
    }
}

// Starlark requires a Display impl, but we just use Debug for that as well,
// since thrift structs don't implement Display
impl<T> std::fmt::Display for Struct<T>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl<'v, T> StarlarkValue<'v> for Struct<T>
where
    T: Debug + 'static,
{
    starlark_type!("ThriftStruct");

    fn matches_type(&self, ty: &str) -> bool {
        std::any::type_name::<T>() == ty
    }

    fn dir_attr(&self) -> Vec<String> {
        self.0.fields.keys().cloned().collect()
    }

    fn get_attr(&self, a: &str, heap: &'v Heap) -> Option<Value<'v>> {
        self.0
            .fields
            .get(a)
            .map(|v| heap.alloc(v.clone()))
            // there is no way to tell if a field is optional and was
            // set to None or just doesn't exist, so just always return
            // None if a field is unknown to avoid AttributeErrors when
            // a field is optional
            .or_else(|| Some(Value::new_none()))
    }
}

impl<'v, T> AllocValue<'v> for Struct<T>
where
    T: Debug + 'static,
{
    fn alloc_value(self, heap: &'v Heap) -> Value<'v> {
        heap.alloc_simple(self)
    }
}

impl<T> AllocFrozenValue for Struct<T>
where
    T: Debug + 'static,
{
    fn alloc_frozen_value(self, heap: &FrozenHeap) -> FrozenValue {
        heap.alloc_simple(self)
    }
}

unsafe impl<T> ProvidesStaticType for Struct<T>
where
    T: 'static,
{
    type StaticType = Struct<T>;
}

// Starlark requires a Display impl, but we just use Debug for that as well,
// since thrift structs don't implement Display
impl std::fmt::Display for ThriftValue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        if let Some(i) = self.as_int() {
            write!(f, "{}", i)
        } else {
            match self {
                Self::String(s) => write!(f, "{}", s),
                _ => write!(f, "{:?}", self),
            }
        }
    }
}

impl<'v> StarlarkValue<'v> for ThriftValue {
    starlark_type!("ThriftValue");

    fn matches_type(&self, ty: &str) -> bool {
        let me = match self {
            Self::Bool(_) => "bool",
            Self::Float(_) | Self::Double(_) => "float",
            Self::Byte(_) | Self::I16(_) | Self::I32(_) | Self::I64(_) => "int",
            Self::String(_) => "string",
            Self::Struct(_) => "struct",
            Self::Map(_) => "dict",
            Self::List(_) => "list",
            Self::Set(_) => "tuple",
            Self::Binary(_) => "binary",
        };
        me == ty
    }

    fn dir_attr(&self) -> Vec<String> {
        match self {
            Self::Struct(s) => s.fields.keys().cloned().collect(),
            _ => unimplemented!("only struct supports dir_attr"),
        }
    }

    fn get_attr(&self, a: &str, heap: &'v Heap) -> Option<Value<'v>> {
        match self {
            Self::Struct(s) => {
                s.fields
                    .get(a)
                    .map(|v| heap.alloc(v.clone()))
                    // there is no way to tell if a field is optional and was
                    // set to None or just doesn't exist, so just always return
                    // None if a field is unknown to avoid AttributeErrors when
                    // a field is optional
                    .or_else(|| Some(Value::new_none()))
            }
            _ => unimplemented!("only struct supports get_attr"),
        }
    }
}

impl<'v> AllocValue<'v> for ThriftValue {
    fn alloc_value(self, heap: &'v Heap) -> Value<'v> {
        if let Some(i) = self.as_i32() {
            Value::new_int(i)
        } else {
            match self {
                Self::Bool(b) => Value::new_bool(b),
                Self::String(s) => heap.alloc_str(&s).to_value(),
                Self::List(l) => {
                    let values: Vec<_> = l.into_iter().map(|v| heap.alloc(v)).collect();
                    heap.alloc_list(&values)
                }
                Self::Set(s) => {
                    let values: Vec<_> = s.into_iter().map(|v| heap.alloc(v)).collect();
                    heap.alloc_tuple(&values)
                }
                _ => heap.alloc_simple(self),
            }
        }
    }
}

impl AllocFrozenValue for ThriftValue {
    fn alloc_frozen_value(self, heap: &FrozenHeap) -> FrozenValue {
        heap.alloc_simple(self)
    }
}

unsafe impl ProvidesStaticType for ThriftValue {
    type StaticType = ThriftValue;
}

#[cfg(test)]
mod tests {
    use example::Example;
    use example::ListItem;
    use maplit::btreemap;
    use maplit::btreeset;
    use starlark::assert::Assert;
    use starlark::syntax::Dialect;

    use super::*;

    #[test]
    fn test_exposed_to_starlark() -> Result<()> {
        let mut a = Assert::new();
        a.dialect(&Dialect::Standard);
        a.globals_add(|gb| {
            gb.set(
                "input",
                Struct::new(&Example {
                    hello: "world".into(),
                    bin: b"binary data".to_vec(),
                    kv: btreemap! {
                        "foo".into() => "bar".into(),
                    },
                    string_list: vec!["alice".into(), "bob".into()],
                    string_set: btreeset! {"alice".into(), "bob".into()},
                    struct_list: vec![ListItem { key: "baz".into() }],
                    option_set: Some("set".into()),
                    option_unset: None,
                })
                .unwrap(),
            )
        });
        a.eq("type(input)", "\"ThriftStruct\"");
        a.eq("input.hello", "\"world\"");
        a.eq("input.option_set", "\"set\"");
        a.eq("input.option_unset", "None");
        a.eq("input.string_list", "[\"alice\", \"bob\"]");
        a.eq("input.string_set", "(\"alice\", \"bob\")");
        a.eq("input.struct_list[0].key", "\"baz\"");
        Ok(())
    }
}
