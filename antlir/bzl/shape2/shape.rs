/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Support library for generated shape code.

use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;

use serde::Deserializer;

/// We can guarantee that Paths in antlir shapes are strings, so store it as a
/// string internally instead of having to deal with fallible conversions
/// to/from Path
#[derive(
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Clone,
    serde::Serialize,
    serde::Deserialize
)]
pub struct ShapePath(String);

impl ShapePath {
    pub fn as_path(&self) -> &Path {
        Path::new(&self.0)
    }

    pub fn to_path_buf(&self) -> PathBuf {
        self.as_path().to_path_buf()
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl AsRef<Path> for ShapePath {
    fn as_ref(&self) -> &Path {
        self.0.as_ref()
    }
}

impl Deref for ShapePath {
    type Target = Path;

    fn deref(&self) -> &Path {
        Path::new(&self.0)
    }
}

impl AsRef<str> for ShapePath {
    fn as_ref(&self) -> &str {
        self.0.as_ref()
    }
}

impl fbthrift::GetTType for ShapePath {
    const TTYPE: fbthrift::TType = fbthrift::TType::String;
}

impl<P> fbthrift::Serialize<P> for ShapePath
where
    P: fbthrift::ProtocolWriter,
{
    fn write(&self, p: &mut P) {
        self.0.write(p)
    }
}

impl<P> fbthrift::Deserialize<P> for ShapePath
where
    P: fbthrift::ProtocolReader,
{
    fn read(p: &mut P) -> anyhow::Result<Self>
    where
        Self: Sized,
    {
        String::read(p).map(Self)
    }
}

pub fn deserialize_i64<'de, D>(deserializer: D) -> Result<i64, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrI64;

    impl<'de> serde::de::Visitor<'de> for StringOrI64 {
        type Value = i64;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("string or i64")
        }

        fn visit_str<E>(self, value: &str) -> std::result::Result<i64, E>
        where
            E: serde::de::Error,
        {
            std::str::FromStr::from_str(value).map_err(E::custom)
        }

        fn visit_i64<E>(self, value: i64) -> std::result::Result<i64, E>
        where
            E: serde::de::Error,
        {
            Ok(value)
        }

        fn visit_u64<E>(self, value: u64) -> std::result::Result<i64, E>
        where
            E: serde::de::Error,
        {
            value.try_into().map_err(E::custom)
        }
    }

    deserializer.deserialize_any(StringOrI64)
}

pub fn deserialize_optional_i64<'de, D>(deserializer: D) -> Result<Option<i64>, D::Error>
where
    D: Deserializer<'de>,
{
    struct StringOrI64;

    impl<'de> serde::de::Visitor<'de> for StringOrI64 {
        type Value = Option<i64>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("string or i64")
        }

        fn visit_str<E>(self, value: &str) -> std::result::Result<Option<i64>, E>
        where
            E: serde::de::Error,
        {
            std::str::FromStr::from_str(value)
                .map_err(E::custom)
                .map(Some)
        }

        fn visit_i64<E>(self, value: i64) -> std::result::Result<Option<i64>, E>
        where
            E: serde::de::Error,
        {
            Ok(Some(value))
        }

        fn visit_u64<E>(self, value: u64) -> std::result::Result<Option<i64>, E>
        where
            E: serde::de::Error,
        {
            value.try_into().map_err(E::custom).map(Some)
        }

        fn visit_none<E>(self) -> std::result::Result<Option<i64>, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }

        fn visit_unit<E>(self) -> std::result::Result<Option<i64>, E>
        where
            E: serde::de::Error,
        {
            Ok(None)
        }
    }

    deserializer.deserialize_any(StringOrI64)
}
