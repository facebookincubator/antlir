/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Write;

use serde::ser::Impossible;
use serde::ser::SerializeStruct;
use serde::ser::Serializer;
use serde::Serialize;

use crate::ser::Error;
use crate::ser::Result;
use crate::ser::UnsupportedValue;
use crate::ser::ValueSerializer;

pub struct SectionSerializer<'a, W>(pub(crate) &'a mut W);

impl<'a, W> SerializeStruct for SectionSerializer<'a, W>
where
    W: Write,
{
    type Ok = ();

    type Error = Error;

    fn serialize_field<T: ?Sized>(&mut self, key: &'static str, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        write!(self.0, "{}=", key)?;
        value.serialize(&mut ValueSerializer(self.0, key))?;
        writeln!(self.0)?;
        Ok(())
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, W> Serializer for SectionSerializer<'a, W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    type SerializeStruct = Self;

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        Ok(self)
    }

    type SerializeSeq = Impossible<Self::Ok, Self::Error>;
    type SerializeTuple = Impossible<Self::Ok, Self::Error>;
    type SerializeTupleStruct = Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = Impossible<Self::Ok, Self::Error>;
    type SerializeMap = Impossible<Self::Ok, Self::Error>;

    fn serialize_bool(self, _v: bool) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_i8(self, _v: i8) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_i16(self, _v: i16) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_i32(self, _v: i32) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_i64(self, _v: i64) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_u8(self, _v: u8) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_u16(self, _v: u16) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_u32(self, _v: u32) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_u64(self, _v: u64) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_f32(self, _v: f32) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_f64(self, _v: f64) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_char(self, _v: char) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_str(self, _v: &str) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_none(self) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_some<T: ?Sized + Serialize>(self, value: &T) -> Result<()> {
        value.serialize(self)
    }

    fn serialize_unit(self) -> Result<()> {
        self.serialize_none()
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        self.serialize_none()
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
        Err(Error::TopLevelSetting)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_tuple(self, _len: usize) -> Result<Self::SerializeTuple> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_tuple_struct(
        self,
        _name: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleStruct> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_tuple_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        Err(UnsupportedValue::NestedSection.into())
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(Error::TopLevelSetting)
    }
}
