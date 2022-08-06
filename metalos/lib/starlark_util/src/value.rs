/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! [Value] abstraction to treat thrift structs generically.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::panic::RefUnwindSafe;

use anyhow::Error;
use anyhow::Result;
use fbthrift::MessageType;
use fbthrift::ProtocolWriter;
use fbthrift::Serialize as ThriftSerialize;
use fbthrift::TType;
use itertools::Itertools;
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub enum Value {
    Bool(bool),
    Byte(i8),
    Float(Float),
    Double(Double),
    I16(i16),
    I32(i32),
    I64(i64),
    String(String),
    Struct(Struct),
    Map(BTreeMap<Value, Value>),
    Set(BTreeSet<Value>),
    List(Vec<Value>),
    Binary(Vec<u8>),
}

impl Value {
    pub fn as_int(&self) -> Option<i64> {
        match self {
            Self::Byte(i) => Some(*i as i64),
            Self::I16(i) => Some(*i as i64),
            Self::I32(i) => Some(*i as i64),
            Self::I64(i) => Some(*i),
            _ => None,
        }
    }

    pub fn as_i32(&self) -> Option<i32> {
        match self {
            Self::Byte(i) => Some(*i as i32),
            Self::I16(i) => Some(*i as i32),
            Self::I32(i) => Some(*i as i32),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Float(f32);
impl From<f32> for Float {
    fn from(f: f32) -> Self {
        Self(f)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize)]
#[repr(transparent)]
#[serde(transparent)]
pub struct Double(f64);
impl From<f64> for Double {
    fn from(f: f64) -> Self {
        Self(f)
    }
}

// Technically these are not *correct*, because float's NaN != NaN, but for our
// purposes let's just say it is
impl Ord for Float {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}
impl Eq for Float {}
impl Ord for Double {
    fn cmp(&self, other: &Self) -> Ordering {
        self.partial_cmp(other).unwrap_or(Ordering::Equal)
    }
}
impl Eq for Double {}

macro_rules! value_from {
    ($t:ty, $v:ident) => {
        impl From<$t> for Value {
            fn from(x: $t) -> Value {
                Value::$v(x)
            }
        }
    };
    ($t:ty, $v:ident, into) => {
        impl From<$t> for Value {
            fn from(x: $t) -> Value {
                Value::$v(x.into())
            }
        }
    };
}

value_from!(bool, Bool);
value_from!(i8, Byte);
value_from!(f32, Float, into);
value_from!(f64, Double, into);
value_from!(i16, I16);
value_from!(i32, I32);
value_from!(i64, I64);
value_from!(String, String);
value_from!(Struct, Struct);
value_from!(Vec<u8>, Binary);
value_from!(BTreeSet<Value>, Set);
value_from!(BTreeMap<Value, Value>, Map);
value_from!(Vec<Value>, List);

impl From<&str> for Value {
    fn from(s: &str) -> Value {
        s.to_string().into()
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Struct {
    ty: String,
    pub fields: BTreeMap<String, Value>,
}

#[derive(Debug)]
enum Item {
    Value(Value),
    Struct(Struct),
    StructField(String),
    ListBegin,
    SetBegin,
    MapBegin,
}

pub struct Protocol {
    stack: Vec<Item>,
}

impl Protocol {
    fn new() -> Self {
        Self { stack: vec![] }
    }
}

/// Thrift Protocol to "serialize" into [Value]s. In theory all the `panic!`s
/// here are impossible, since thrift codegen will be well-formed, but
/// [to_value] will catch these panics and convert them into proper [Error]s.
impl ProtocolWriter for Protocol {
    type Final = Value;

    fn write_struct_begin(&mut self, ty: &str) {
        self.stack.push(Item::Struct(Struct {
            ty: ty.to_string(),
            fields: BTreeMap::new(),
        }))
    }

    fn write_struct_end(&mut self) {
        let top = self.stack.pop().expect("struct_end: stack empty");
        match top {
            Item::Struct(s) => self.stack.push(Item::Value(s.into())),
            _ => panic!("struct_end: not a struct"),
        }
    }

    fn write_field_begin(&mut self, name: &str, _: TType, _: i16) {
        let top = self.stack.last().expect("field_begin: stack empty");
        match top {
            &Item::Struct(_) => {}
            _ => panic!("field_begin: not struct"),
        }
        self.stack.push(Item::StructField(name.to_string()));
    }

    fn write_field_end(&mut self) {
        let value = self.stack.pop().expect("field_end: no value");
        let value = match value {
            Item::Value(v) => v,
            _ => panic!("field_end: not a value"),
        };
        let field = self.stack.pop().expect("field_end: no field");
        let field = match field {
            Item::StructField(f) => f,
            _ => panic!("field_end: not a field"),
        };
        let strct = self.stack.last_mut().expect("field_end: no struct");
        match strct {
            Item::Struct(ref mut s) => s.fields.insert(field, value),
            _ => panic!("field_end: not a struct"),
        };
    }

    // written when the last field is done, it is followed by struct_end so
    // nothing needs to be done
    fn write_field_stop(&mut self) {}

    fn write_map_begin(&mut self, _: TType, _: TType, _: usize) {
        self.stack.push(Item::MapBegin);
    }
    fn write_map_key_begin(&mut self) {}
    fn write_map_value_begin(&mut self) {}
    fn write_map_end(&mut self) {
        // pop all the values off the stack until reaching the Map
        let mut values = vec![];
        loop {
            let top = self
                .stack
                .pop()
                .expect("map_end: stack exhausted before reaching map_begin");
            match top {
                Item::MapBegin => {
                    // keys are pushed first, then values, so reverse it
                    // after popping
                    let mut map = BTreeMap::new();
                    for (v, k) in values.into_iter().tuples() {
                        map.insert(k, v);
                    }
                    self.stack.push(Item::Value(map.into()));
                    return;
                }
                Item::Value(v) => values.push(v),
                _ => panic!("map_end: neither value nor map"),
            }
        }
    }

    fn write_list_begin(&mut self, _: TType, _: usize) {
        self.stack.push(Item::ListBegin);
    }
    fn write_list_value_begin(&mut self) {}
    fn write_list_end(&mut self) {
        // pop all the values off the stack until reaching the List
        let mut values = Vec::<Value>::new();
        loop {
            let top = self
                .stack
                .pop()
                .expect("list_end: stack exhausted before reaching list_begin");
            match top {
                Item::ListBegin => {
                    values.reverse();
                    self.stack.push(Item::Value(values.into()));
                    return;
                }
                Item::Value(v) => values.push(v),
                _ => panic!("list_end: neither value nor list"),
            }
        }
    }

    fn write_set_begin(&mut self, _: TType, _: usize) {
        self.stack.push(Item::SetBegin);
    }
    fn write_set_value_begin(&mut self) {}
    fn write_set_end(&mut self) {
        // pop all the values off the stack until reaching the Set
        let mut values = vec![];
        loop {
            let top = self
                .stack
                .pop()
                .expect("set_end: stack exhausted before reaching set_begin");
            match top {
                Item::SetBegin => {
                    let mut set = BTreeSet::new();
                    for v in values {
                        set.insert(v);
                    }
                    self.stack.push(Item::Value(set.into()));
                    return;
                }
                Item::Value(v) => values.push(v),
                _ => panic!("set_end: neither value nor set"),
            }
        }
    }

    fn write_bool(&mut self, b: bool) {
        self.stack.push(Item::Value(b.into()))
    }
    fn write_byte(&mut self, i: i8) {
        self.stack.push(Item::Value(i.into()))
    }
    fn write_i16(&mut self, i: i16) {
        self.stack.push(Item::Value(i.into()))
    }
    fn write_i32(&mut self, i: i32) {
        self.stack.push(Item::Value(i.into()))
    }
    fn write_i64(&mut self, i: i64) {
        self.stack.push(Item::Value(i.into()))
    }
    fn write_double(&mut self, f: f64) {
        self.stack.push(Item::Value(f.into()))
    }
    fn write_float(&mut self, f: f32) {
        self.stack.push(Item::Value(f.into()))
    }
    fn write_string(&mut self, s: &str) {
        self.stack.push(Item::Value(s.to_string().into()))
    }
    fn write_binary(&mut self, b: &[u8]) {
        self.stack.push(Item::Value(b.to_vec().into()))
    }

    fn write_message_begin(&mut self, _: &str, _: MessageType, _: u32) {
        panic!("ValueProtocol does not support Thrift Messages")
    }
    fn write_message_end(&mut self) {
        panic!("ValueProtocol does not support Thrift Messages")
    }

    fn finish(mut self) -> <Self as ProtocolWriter>::Final {
        assert_eq!(self.stack.len(), 1);
        match self.stack.remove(0) {
            Item::Value(v) => v,
            _ => panic!("finish: not a value"),
        }
    }
}

pub fn to_value<T>(t: &T) -> Result<Value>
where
    T: ThriftSerialize<Protocol> + RefUnwindSafe,
{
    let mut p = Protocol::new();
    match std::panic::catch_unwind(|| {
        t.write(&mut p);
        p.finish()
    }) {
        Ok(v) => Ok(v),
        Err(panic) => Err(match panic.downcast::<String>() {
            Ok(s) => Error::msg(s),
            Err(_) => Error::msg("panicked while serializing to value"),
        }),
    }
}

#[cfg(test)]
mod tests {
    use example::Example;
    use example::ListItem;
    use maplit::btreemap;
    use maplit::btreeset;

    use super::*;

    #[test]
    fn test_to_value() -> Result<()> {
        assert_eq!(
            Value::Struct(Struct {
                ty: "Example".into(),
                fields: btreemap! {
                    "hello".into() => "world".into(),
                    "bin".into() => b"binary data".to_vec().into(),
                    "kv".into() => btreemap!{
                        "foo".into() => "bar".into(),
                    }.into(),
                    "string_list".into() => vec![Value::from("alice"), Value::from("bob")].into(),
                    "string_set".into() =>  btreeset! {Value::from("alice"), Value::from("bob")}.into(),
                    "struct_list".into() => vec![Value::Struct(Struct {
                        ty: "ListItem".into(),
                        fields: btreemap!{"key".into() => "baz".into() }
                    })].into(),
                    "option_set".into() => "set".into(),
                    // annoyingly, option_unset never even gets serialized, but
                    // I can live with that...
                }
            }),
            to_value(&Example {
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
            })?
        );
        Ok(())
    }
}
