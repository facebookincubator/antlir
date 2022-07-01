/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Display;
use std::io::Cursor;
use std::io::Write;

use serde::ser;
use serde::Serialize;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum UnsupportedValue {
    #[error("sections can only exist at the top level")]
    NestedSection,
    #[error("all fields must be explicitly specified in structs")]
    Map,
    #[error("raw byte sequences cannot be used as a value")]
    Bytes,
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("settings are not supported at the top level")]
    TopLevelSetting,
    #[error("keys must all be strings")]
    NonStringKey,
    #[error(transparent)]
    UnsupportedValue(#[from] UnsupportedValue),
    #[error("{0}")]
    Custom(String),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

impl ser::Error for Error {
    fn custom<T>(msg: T) -> Self
    where
        T: Display,
    {
        Self::Custom(msg.to_string())
    }
}

mod key;
mod section;
mod unit;
mod unit_file;
mod value;
mod value_seq;
use section::*;
use unit::*;
use unit_file::*;
use value::*;
use value_seq::*;

pub fn to_writer<W, T>(w: W, value: &T) -> Result<()>
where
    W: Write,
    T: Serialize,
{
    let mut serializer = UnitFileSerializer(w);
    value.serialize(&mut serializer)
}

pub fn to_bytes<T>(value: &T) -> Result<Vec<u8>>
where
    T: Serialize,
{
    let mut cursor = Cursor::new(Vec::new());
    to_writer(&mut cursor, value)?;
    Ok(cursor.into_inner())
}

pub fn to_string<T>(value: &T) -> Result<String>
where
    T: Serialize,
{
    let bytes = to_bytes(value)?;
    let s = unsafe {
        // We never emit invalid UTF-8
        String::from_utf8_unchecked(bytes)
    };
    Ok(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Serialize)]
    #[serde(rename_all = "PascalCase")]
    struct UnitSection {
        description: String,
    }

    #[test]
    fn simple_struct() {
        #[derive(Serialize)]
        #[serde(rename_all = "PascalCase")]
        struct ServiceSection {
            exec_start_pre: Vec<String>,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "PascalCase")]
        struct Unit {
            unit: UnitSection,
            service: ServiceSection,
        }

        let test = Unit {
            unit: UnitSection {
                description: "demo service".to_string(),
            },
            service: ServiceSection {
                exec_start_pre: vec![
                    "/bin/echo hello".to_string(),
                    "/bin/echo goodbye".to_string(),
                ],
            },
        };
        let expected = "[Unit]\nDescription=demo service\n[Service]\nExecStartPre=/bin/echo hello\nExecStartPre=/bin/echo goodbye\n\n";
        assert_eq!(to_string(&test).unwrap(), expected);
    }

    #[test]
    fn nested_sections_not_allowed() {
        #[derive(Serialize)]
        #[serde(rename_all = "PascalCase")]
        struct ServiceSection {
            exec_start_pre: Vec<String>,
            illegal_nested: UnitSection,
        }

        #[derive(Serialize)]
        #[serde(rename_all = "PascalCase")]
        struct Unit {
            unit: UnitSection,
            service: ServiceSection,
        }

        let test = Unit {
            unit: UnitSection {
                description: "demo service".to_string(),
            },
            service: ServiceSection {
                exec_start_pre: vec![
                    "/bin/echo hello".to_string(),
                    "/bin/echo goodbye".to_string(),
                ],
                illegal_nested: UnitSection {
                    description: "demo service".to_string(),
                },
            },
        };
        match to_string(&test) {
            Ok(_) => panic!("nested structs should fail"),
            Err(e) => match e {
                Error::UnsupportedValue(UnsupportedValue::NestedSection) => {}
                _ => panic!("expected NestedSection, got {:?}", e),
            },
        }
    }

    #[test]
    fn top_level_keys_not_allowed() {
        #[derive(Serialize)]
        #[serde(rename_all = "PascalCase")]
        struct Unit {
            unit: UnitSection,
            setting: String,
        }

        let test = Unit {
            unit: UnitSection {
                description: "demo service".to_string(),
            },
            setting: "I am a top level setting".to_string(),
        };
        match to_string(&test) {
            Ok(_) => panic!("top-level setting should fail"),
            Err(e) => match e {
                Error::TopLevelSetting => {}
                _ => panic!("expected TopLevelSetting, got {:?}", e),
            },
        }
    }
}
