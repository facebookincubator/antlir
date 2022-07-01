/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
#![allow(non_snake_case)]
#![allow(non_camel_case_types)]

use anyhow::Context;
use anyhow::Result;
use fbthrift::protocol::ProtocolReader;
use fbthrift::protocol::ProtocolWriter;
use fbthrift::ttype::GetTType;
use fbthrift::ttype::TType;
use serde::Deserialize;
use serde::Serialize;

use shape::ShapePath;

#[derive(Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct target_t {
    pub name: String,
    pub path: ShapePath,
}

impl GetTType for target_t {
    const TTYPE: TType = TType::Struct;
}

impl<P> fbthrift::Serialize<P> for target_t
where
    P: ProtocolWriter,
{
    #[deny(unused_variables)]
    fn write(&self, p: &mut P) {
        p.write_struct_begin("target_t");
        let Self { name, path } = self;
        p.write_field_begin("name", TType::String, 1);
        p.write_string(name);
        p.write_field_end();
        p.write_field_begin("path", TType::String, 2);
        p.write_string(path.as_str());
        p.write_field_end();
        p.write_field_stop();
        p.write_struct_end();
    }
}

impl<P> fbthrift::Deserialize<P> for target_t
where
    P: ProtocolReader,
{
    fn read(p: &mut P) -> Result<Self> {
        static FIELDS: &[::fbthrift::Field] = &[
            ::fbthrift::Field::new("name", TType::String, 1),
            ::fbthrift::Field::new("path", TType::String, 2),
        ];
        let mut field_name = None;
        let mut field_path = None;
        let _ = p.read_struct_begin(|_| ())?;
        loop {
            let (_, fty, fid) = p.read_field_begin(|_| (), FIELDS)?;
            match (fty, fid) {
                (::fbthrift::TType::Stop, _) => break,
                (::fbthrift::TType::String, 1) => {
                    field_name = Some(fbthrift::Deserialize::read(p)?);
                }
                (::fbthrift::TType::String, 2) => {
                    field_path = Some(fbthrift::Deserialize::read(p)?);
                }
                (fty, _) => p.skip(fty)?,
            }
            p.read_field_end()?;
        }
        p.read_struct_end()?;
        Ok(Self {
            name: field_name.context("missing 'name'")?,
            path: field_path.context("missing 'path'")?,
        })
    }
}
