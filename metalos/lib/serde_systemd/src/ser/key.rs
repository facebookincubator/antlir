/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Write;

use serde::ser::Impossible;
use serde::ser::Serializer;
use serde::Serialize;

use crate::ser::Error;
use crate::ser::Result;

pub struct KeySerializer<'a, W>(pub(crate) &'a mut W);

impl<'a, W> Serializer for &'a mut KeySerializer<'a, W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    // Keys can only be strings, so this is the only method that is implemented
    fn serialize_str(self, v: &str) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    // Fine, allow single characters too, it's just a short string
    fn serialize_char(self, v: char) -> Result<()> {
        self.serialize_str(&v.to_string())
    }

    // Maybe this is a string, give it a shot
    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<()> {
        value.serialize(self)
    }

    type SerializeSeq = Impossible<Self::Ok, Self::Error>;
    type SerializeTuple = Impossible<Self::Ok, Self::Error>;
    type SerializeTupleStruct = Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = Impossible<Self::Ok, Self::Error>;
    type SerializeMap = Impossible<Self::Ok, Self::Error>;
    type SerializeStruct = Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = Impossible<Self::Ok, Self::Error>;

    fn serialize_bool(self, _v: bool) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_i8(self, _v: i8) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_i16(self, _v: i16) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_i32(self, _v: i32) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_i64(self, _v: i64) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_u8(self, _v: u8) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_u16(self, _v: u16) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_u32(self, _v: u32) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_u64(self, _v: u64) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_f32(self, _v: f32) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_f64(self, _v: f64) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_none(self) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_unit(self) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        variant: &'static str,
    ) -> Result<()> {
        self.serialize_str(variant)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        value: &T,
    ) -> Result<()> {
        value.serialize(self)
    }

    fn serialize_newtype_variant<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _value: &T,
    ) -> Result<()> {
        Err(Error::NonStringKey)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(Error::NonStringKey)
    }

    fn serialize_tuple(self, len: usize) -> Result<Self::SerializeTuple> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        self.serialize_seq(Some(len))
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(Error::NonStringKey)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        Err(Error::NonStringKey)
    }

    fn serialize_struct(self, _name: &'static str, len: usize) -> Result<Self::SerializeStruct> {
        self.serialize_map(Some(len))
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(Error::NonStringKey)
    }
}
