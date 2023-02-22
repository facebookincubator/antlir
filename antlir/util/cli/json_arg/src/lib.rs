/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! CLIs (especially those designed to be called by automation) often take
//! structured data as inputs. This ends up creating a ton of boilerplate where
//! a CLI arg is declared as a [PathBuf] and then is quickly opened, read from
//! and deserialized with some [serde] format crate.
//! This crate provides two newtype wrappers ([Serde] and [SerdeFile]) that can
//! deserialize any Serde-compatible arguments with no extra effort.

use std::cmp::Ordering;
use std::fmt::Debug;
use std::fmt::Display;
use std::hash::Hash;
use std::hash::Hasher;
use std::io::Cursor;
use std::io::Read;
use std::marker::PhantomData;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use serde::Deserialize;

pub trait DeserializeReader<T> {
    type Error;

    fn deserialize<R: Read>(reader: R) -> Result<T, Self::Error>;
}

/// Deserialize the argument as JSON
pub struct JsonFormat;

impl<'de, T> DeserializeReader<T> for JsonFormat
where
    T: Deserialize<'de>,
{
    type Error = serde_json::Error;

    fn deserialize<R: Read>(reader: R) -> Result<T, Self::Error> {
        let mut deser = serde_json::Deserializer::from_reader(reader);
        T::deserialize(&mut deser)
    }
}

/// Deserialize the argument as TOML
pub struct TomlFormat;

impl<T> DeserializeReader<T> for TomlFormat
where
    T: for<'de> Deserialize<'de>,
{
    type Error = std::io::Error;

    fn deserialize<R: Read>(reader: R) -> Result<T, Self::Error> {
        // [toml] has no way to deserialize from a reader :(
        let str = std::io::read_to_string(reader)?;
        toml::from_str(&str)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

/// Inline JSON string. The argument provided by the caller is the raw JSON
/// string (and the caller must consequently deal with shell quoting
/// ahead-of-time).
pub struct Serde<T, D>(T, PhantomData<D>);

pub type Json<T> = Serde<T, JsonFormat>;
pub type Toml<T> = Serde<T, TomlFormat>;

impl<'de, T, D> FromStr for Serde<T, D>
where
    T: Deserialize<'de>,
    D: DeserializeReader<T>,
{
    type Err = D::Error;

    fn from_str(s: &str) -> Result<Self, D::Error> {
        D::deserialize(Cursor::new(s)).map(|v| Self(v, PhantomData))
    }
}

impl<T, D> Deref for Serde<T, D> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T, D> DerefMut for Serde<T, D> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T, D> Debug for Serde<T, D>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        Debug::fmt(&self.0, f)
    }
}

impl<T, D> Clone for Serde<T, D>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self(self.0.clone(), PhantomData)
    }
}

impl<T, D> Serde<T, D> {
    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }

    #[inline]
    pub fn as_inner(&self) -> &T {
        &self.0
    }
}

/// Argument that represents a serialized file. The argument provided by the
/// caller is the path to the file that is deserialized immediately on load.
/// The original path is preserved and accessible with [SerdeFile::path]
pub struct SerdeFile<T, D> {
    value: T,
    path: PathBuf,
    deser: PhantomData<D>,
}

pub type JsonFile<T> = SerdeFile<T, JsonFormat>;
pub type TomlFile<T> = SerdeFile<T, TomlFormat>;

impl<'de, T, D> FromStr for SerdeFile<T, D>
where
    T: Deserialize<'de>,
    D: DeserializeReader<T>,
    D::Error: Display,
{
    type Err = std::io::Error;

    fn from_str(path: &str) -> std::io::Result<Self> {
        let f = std::fs::File::open(path)?;
        D::deserialize(f)
            .map(|value| Self {
                path: path.into(),
                value,
                deser: PhantomData,
            })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

impl<T, D> Deref for SerdeFile<T, D> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T, D> DerefMut for SerdeFile<T, D> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<T, D> Debug for SerdeFile<T, D>
where
    T: Debug,
{
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SerdeFile")
            .field("path", &self.path)
            .field("value", &self.value)
            .finish()
    }
}

impl<T, D> Clone for SerdeFile<T, D>
where
    T: Clone,
{
    fn clone(&self) -> Self {
        Self {
            path: self.path.clone(),
            value: self.value.clone(),
            deser: PhantomData,
        }
    }
}

impl<T, D> SerdeFile<T, D> {
    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }

    #[inline]
    pub fn as_inner(&self) -> &T {
        self
    }

    #[inline]
    pub fn into_inner(self) -> T {
        self.value
    }
}

macro_rules! common_impl {
    ($i:ident) => {
        impl<T, D> AsRef<T> for $i<T, D> {
            #[inline]
            fn as_ref(&self) -> &T {
                self
            }
        }

        impl<T, D> PartialEq for $i<T, D>
        where
            T: PartialEq,
        {
            fn eq(&self, rhs: &Self) -> bool {
                self.as_inner() == rhs.as_inner()
            }
        }

        impl<T, D> PartialEq<T> for $i<T, D>
        where
            T: PartialEq,
        {
            fn eq(&self, rhs: &T) -> bool {
                self.as_inner() == rhs
            }
        }

        impl<T, D> Eq for $i<T, D> where T: Eq {}

        impl<T, D> PartialOrd for $i<T, D>
        where
            T: PartialOrd,
        {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                self.as_inner().partial_cmp(other.as_inner())
            }
        }

        impl<T, D> PartialOrd<T> for $i<T, D>
        where
            T: PartialOrd,
        {
            fn partial_cmp(&self, other: &T) -> Option<Ordering> {
                self.as_inner().partial_cmp(other)
            }
        }

        impl<T, D> Ord for $i<T, D>
        where
            T: Ord,
        {
            fn cmp(&self, other: &Self) -> Ordering {
                self.as_inner().cmp(other.as_inner())
            }
        }

        impl<T, D> Hash for $i<T, D>
        where
            T: Hash,
        {
            fn hash<H>(&self, state: &mut H)
            where
                H: Hasher,
            {
                self.as_inner().hash(state)
            }
        }
    };
}

common_impl!(Serde);
common_impl!(SerdeFile);

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;

    use clap::Parser;
    use serde::Serialize;
    use similar_asserts::assert_eq;
    use tempfile::NamedTempFile;

    use super::*;

    #[derive(Debug, PartialEq, Deserialize, Serialize)]
    struct Example {
        foo: String,
        bar: u32,
    }

    #[derive(Debug, Parser)]
    struct Args {
        #[clap(long)]
        inline: Option<Json<Example>>,
        #[clap(long)]
        file: Option<JsonFile<Example>>,
    }

    #[test]
    fn inline() {
        let example = Example {
            foo: "baz".into(),
            bar: 42,
        };
        let inline_str = serde_json::to_string(&example).expect("failed to serialize");
        let args = Args::parse_from(vec!["inline", "--inline", &inline_str]);
        assert_eq!(args.inline.expect("definitely here"), example,);
    }

    #[test]
    fn file() {
        let example = Example {
            foo: "baz".into(),
            bar: 42,
        };
        let mut tmp = NamedTempFile::new().expect("failed to create tmp file");
        serde_json::to_writer(&mut tmp, &example).expect("failed to serialize");
        let args = Args::parse_from(vec![
            OsStr::new("file"),
            OsStr::new("--file"),
            tmp.path().as_os_str(),
        ]);
        assert_eq!(args.file.expect("definitely here"), example);
    }
}
