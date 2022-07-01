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
use crate::ser::UnsupportedValue;
use crate::ser::ValueSeqSerializer;

/// ValueSerializer is used to write out values for unit file settings.
pub struct ValueSerializer<'a, W>(pub(crate) &'a mut W, pub(crate) &'static str);

impl<'a, W> Serializer for &'a mut ValueSerializer<'a, W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    type SerializeSeq = ValueSeqSerializer<'a, W>;
    type SerializeTuple = ValueSeqSerializer<'a, W>;
    type SerializeTupleStruct = ValueSeqSerializer<'a, W>;
    type SerializeTupleVariant = ValueSeqSerializer<'a, W>;

    fn serialize_bool(self, v: bool) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_i8(self, v: i8) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_i16(self, v: i16) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_i32(self, v: i32) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_i64(self, v: i64) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_u8(self, v: u8) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_u16(self, v: u16) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_u32(self, v: u32) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_u64(self, v: u64) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_f32(self, v: f32) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_f64(self, v: f64) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_char(self, v: char) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_str(self, v: &str) -> Result<()> {
        write!(self.0, "{}", v)?;
        Ok(())
    }

    fn serialize_bytes(self, _v: &[u8]) -> Result<()> {
        Err(UnsupportedValue::Bytes.into())
    }

    fn serialize_none(self) -> Result<()> {
        Ok(())
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
        value: &T,
    ) -> Result<()> {
        value.serialize(self)
    }

    fn serialize_seq(self, _len: Option<usize>) -> Result<Self::SerializeSeq> {
        Ok(ValueSeqSerializer::new(self.0, self.1))
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
        len: usize,
    ) -> Result<Self::SerializeTupleVariant> {
        self.serialize_seq(Some(len))
    }

    // The following all map to sections, which cannot be nested, so are banned
    // here
    type SerializeMap = Impossible<Self::Ok, Self::Error>;
    type SerializeStruct = Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = Impossible<Self::Ok, Self::Error>;

    fn serialize_map(self, _len: Option<usize>) -> Result<Self::SerializeMap> {
        Err(UnsupportedValue::NestedSection.into())
    }

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        Err(UnsupportedValue::NestedSection.into())
    }

    fn serialize_struct_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
        _len: usize,
    ) -> Result<Self::SerializeStructVariant> {
        Err(UnsupportedValue::NestedSection.into())
    }
}
