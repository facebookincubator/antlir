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
use crate::ser::UnitSerializer;
use crate::ser::UnsupportedValue;

/// UnitFileSerializer is the main entrypoint to writing out a unit file. It
/// only supports serializing a single top-level struct, using [UnitSerializer]
/// internally.
pub struct UnitFileSerializer<W>(pub(crate) W);

impl<W> UnitFileSerializer<W> {
    pub fn new(w: W) -> Self {
        Self(w)
    }

    pub fn into_inner(self) -> W {
        self.0
    }
}

impl<'a, W> Serializer for &'a mut UnitFileSerializer<W>
where
    W: Write,
{
    type Ok = ();

    type Error = Error;

    type SerializeStruct = UnitSerializer<'a, W>;

    fn serialize_struct(self, _name: &'static str, _len: usize) -> Result<Self::SerializeStruct> {
        Ok(UnitSerializer(&mut self.0))
    }

    // UnitFileSerializer does not serialize any primitive types, or anything
    // other than top-level structs

    // It may be technically possible to implement SerializeMap, but there is
    // little value in allowing even the top level unit file contents be loosely
    // typed
    type SerializeMap = Impossible<Self::Ok, Self::Error>;
    type SerializeSeq = Impossible<Self::Ok, Self::Error>;
    type SerializeTuple = Impossible<Self::Ok, Self::Error>;
    type SerializeTupleStruct = Impossible<Self::Ok, Self::Error>;
    type SerializeTupleVariant = Impossible<Self::Ok, Self::Error>;
    type SerializeStructVariant = Impossible<Self::Ok, Self::Error>;

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

    fn serialize_some<T: ?Sized + Serialize>(self, _value: &T) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_unit(self) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_unit_struct(self, _name: &'static str) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_unit_variant(
        self,
        _name: &'static str,
        _variant_index: u32,
        _variant: &'static str,
    ) -> Result<()> {
        Err(Error::TopLevelSetting)
    }

    fn serialize_newtype_struct<T: ?Sized + Serialize>(
        self,
        _name: &'static str,
        _value: &T,
    ) -> Result<()> {
        Err(Error::TopLevelSetting)
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
        Err(UnsupportedValue::Map.into())
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
