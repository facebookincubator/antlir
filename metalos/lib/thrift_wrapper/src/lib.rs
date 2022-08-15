/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::path::PathBuf;

use anyhow::Context;
use serde::de::Error as _;
use serde::ser::Error as _;
use serde::Deserialize;
use serde::Deserializer;
use serde::Serialize;
use serde::Serializer;
use serde_with::DeserializeAs;
use serde_with::SerializeAs;
use thiserror::Error;
pub use thrift_wrapper_derive::thrift_server;
#[doc(hidden)]
pub use thrift_wrapper_derive::ThriftWrapper;

/// Re-export some dependencies for the proc-macro to use without having to be
/// added as a dep to every user crate.
#[doc(hidden)]
pub mod __deps {
    pub use anyhow;
    pub use async_trait;
    pub use fbthrift;
    pub use serde;
    pub use serde_with;
}

#[derive(Debug, Error)]
pub enum Error {
    #[error("error in field {field}: {error}")]
    Nested { field: String, error: Box<Error> },
    #[error("expected package of kind {expected}, actual was {actual}")]
    PackageKind {
        expected: &'static str,
        actual: String,
    },
    #[error("unrecognized enum variant {0}")]
    Enum(String),
    #[error("unrecognized union variant {0}")]
    Union(i32),
    #[error(transparent)]
    Other(#[from] anyhow::Error),
    #[error("infallible conversion failed - impossible")]
    Infallible(#[from] std::convert::Infallible),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Easily add context to an [Error] to enable nested field tracking to show the
/// user exactly what field caused an error.
pub trait FieldContext<T> {
    fn in_field(self, field: &str) -> Result<T>;
}

impl<T> FieldContext<T> for std::result::Result<T, Error> {
    fn in_field(self, field: &str) -> Result<T> {
        self.map_err(|err| match err {
            Error::Nested {
                field: nested_field,
                error,
            } => Error::Nested {
                field: format!("{}.{}", field, nested_field),
                error,
            },
            err => Error::Nested {
                field: field.into(),
                error: Box::new(err),
            },
        })
    }
}

/// Trait for a type that has a direct mapping to/from a Thrift type, but with
/// extra guarantees in the Rust type system. Conversion from Thrift is fallible
/// since the underlying Thrift types might not meet whatever criteria is
/// desired in Rust, but conversion back into Thrift is always safe.
pub trait ThriftWrapper: Sized + Clone {
    type Thrift;
    fn from_thrift(thrift: Self::Thrift) -> Result<Self>;
    fn into_thrift(self) -> Self::Thrift;
}

impl ThriftWrapper for uuid::Uuid {
    type Thrift = String;

    fn from_thrift(thrift: Self::Thrift) -> Result<Self> {
        uuid::Uuid::parse_str(&thrift)
            .with_context(|| format!("'{}' is not a valid uuid", thrift))
            .map_err(Error::from)
    }

    fn into_thrift(self) -> Self::Thrift {
        self.to_simple().to_string()
    }
}

impl ThriftWrapper for url::Url {
    type Thrift = String;

    fn from_thrift(thrift: Self::Thrift) -> Result<Self> {
        url::Url::parse(&thrift)
            .with_context(|| format!("'{}' is not a valid url", thrift))
            .map_err(Error::from)
    }

    fn into_thrift(self) -> Self::Thrift {
        self.to_string()
    }
}

impl ThriftWrapper for PathBuf {
    type Thrift = Vec<u8>;

    fn from_thrift(thrift: Vec<u8>) -> Result<Self> {
        Ok(PathBuf::from(OsString::from_vec(thrift)))
    }

    fn into_thrift(self) -> Vec<u8> {
        self.into_os_string().into_vec()
    }
}

// For types that are identical in Rust and Thrift (primitives)
macro_rules! identity_wrapper {
    ($t:ty) => {
        impl ThriftWrapper for $t {
            type Thrift = $t;

            fn from_thrift(thrift: $t) -> Result<Self> {
                Ok(thrift)
            }

            fn into_thrift(self) -> $t {
                self
            }
        }
    };
}

identity_wrapper!(bool);
identity_wrapper!(i8);
identity_wrapper!(i16);
identity_wrapper!(i32);
identity_wrapper!(i64);
identity_wrapper!(f32);
identity_wrapper!(f64);
identity_wrapper!(String);

impl<T> ThriftWrapper for Vec<T>
where
    T: ThriftWrapper,
{
    type Thrift = Vec<T::Thrift>;

    fn from_thrift(thrift: Vec<T::Thrift>) -> Result<Self> {
        thrift.into_iter().map(T::from_thrift).collect()
    }

    fn into_thrift(self) -> Vec<T::Thrift> {
        self.into_iter().map(T::into_thrift).collect()
    }
}

impl<T> ThriftWrapper for Option<T>
where
    T: ThriftWrapper,
{
    type Thrift = Option<T::Thrift>;

    fn from_thrift(thrift: Option<T::Thrift>) -> Result<Self> {
        thrift.map(T::from_thrift).transpose()
    }

    fn into_thrift(self) -> Option<T::Thrift> {
        self.map(T::into_thrift)
    }
}

/// Provides a simple mechanism to include Thrift structures inside Serde
/// structures by serializing to JSON.
pub struct SerdeJsonThrift;

impl<T> SerializeAs<T> for SerdeJsonThrift
where
    T: fbthrift::simplejson_protocol::Serializable,
{
    fn serialize_as<S>(source: &T, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let json_buf = fbthrift::simplejson_protocol::serialize(source);
        let json_value: serde_json::Value =
            serde_json::from_slice(&json_buf).map_err(S::Error::custom)?;
        json_value.serialize(serializer)
    }
}

impl<'de, T> DeserializeAs<'de, T> for SerdeJsonThrift
where
    T: fbthrift::Deserialize<
        fbthrift::simplejson_protocol::SimpleJsonProtocolDeserializer<
            std::io::Cursor<bytes::Bytes>,
        >,
    >,
{
    fn deserialize_as<D>(deserializer: D) -> std::result::Result<T, D::Error>
    where
        D: Deserializer<'de>,
    {
        let json_value = serde_json::Value::deserialize(deserializer).map_err(D::Error::custom)?;
        let json_buf = serde_json::to_vec(&json_value).map_err(D::Error::custom)?;
        fbthrift::simplejson_protocol::deserialize(json_buf).map_err(D::Error::custom)
    }
}
