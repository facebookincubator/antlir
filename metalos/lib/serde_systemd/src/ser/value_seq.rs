/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Write;

use serde::ser::SerializeSeq;
use serde::ser::SerializeTuple;
use serde::ser::SerializeTupleStruct;
use serde::ser::SerializeTupleVariant;
use serde::Serialize;

use crate::ser::Error;
use crate::ser::Result;
use crate::ser::ValueSerializer;

/// ValueSerializer is used to write out values for unit file settings that are
/// repeated multiple times.
/// Sequences with more than one setting repeat the key across multiple lines.
/// List types that require an alternate separator must provide a custom
/// [Serialize] implementation, or use an alternative like
/// `#[serde(serialize_with=..)]`.
pub struct ValueSeqSerializer<'a, W> {
    w: &'a mut W,
    first: bool,
    key: &'static str,
}

impl<'a, W> ValueSeqSerializer<'a, W>
where
    W: Write,
{
    pub fn new(w: &'a mut W, key: &'static str) -> Self {
        Self {
            w,
            first: true,
            key,
        }
    }

    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        if !self.first {
            write!(self.w, "{}=", self.key)?;
        } else {
            self.first = false;
        }
        value.serialize(&mut ValueSerializer(&mut self.w, self.key))?;
        writeln!(self.w)?;
        Ok(())
    }
}

impl<'a, W> SerializeSeq for ValueSeqSerializer<'a, W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        ValueSeqSerializer::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, W> SerializeTuple for ValueSeqSerializer<'a, W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    fn serialize_element<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        ValueSeqSerializer::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, W> SerializeTupleVariant for ValueSeqSerializer<'a, W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        ValueSeqSerializer::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}

impl<'a, W> SerializeTupleStruct for ValueSeqSerializer<'a, W>
where
    W: Write,
{
    type Ok = ();
    type Error = Error;

    fn serialize_field<T: ?Sized>(&mut self, value: &T) -> Result<()>
    where
        T: Serialize,
    {
        ValueSeqSerializer::serialize_element(self, value)
    }

    fn end(self) -> Result<Self::Ok> {
        Ok(())
    }
}
