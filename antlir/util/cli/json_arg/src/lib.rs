/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! CLIs (especially those designed to be called by automation) often take JSON
//! data as inputs. This ends up creating a ton of boilerplate where a CLI arg
//! is declared as a [PathBuf] and then is quickly opened, read from and
//! deserialized with [serde_json].
//! This crate provides two newtype wrappers ([Json] and [JsonFile]) that can
//! deserialize JSON arguments with no extra effort.

use std::cmp::Ordering;
use std::hash::Hash;
use std::hash::Hasher;
use std::io::Cursor;
use std::ops::Deref;
use std::ops::DerefMut;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use serde::Deserialize;

/// Inline JSON string. The argument provided by the caller is the raw JSON
/// string (and the caller must consequently deal with shell quoting
/// ahead-of-time).
#[derive(Debug, Clone)]
pub struct Json<T>(T);

impl<'de, T> FromStr for Json<T>
where
    T: Deserialize<'de>,
{
    type Err = serde_json::Error;

    fn from_str(s: &str) -> serde_json::Result<Self> {
        let mut deser = serde_json::Deserializer::from_reader(Cursor::new(s));
        T::deserialize(&mut deser).map(Self)
    }
}

impl<T> Deref for Json<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl<T> DerefMut for Json<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}

impl<T> Json<T> {
    #[inline]
    pub fn into_inner(self) -> T {
        self.0
    }

    #[inline]
    pub fn as_inner(&self) -> &T {
        &self.0
    }
}

/// Argument that represents a JSON file. The argument provided by the caller is
/// the path to the JSON file that is deserialized immediately on load.
/// The original path is preserved and accessible with [JsonFile::path]
#[derive(Debug, Clone)]
pub struct JsonFile<T> {
    value: T,
    path: PathBuf,
}

impl<'de, T> FromStr for JsonFile<T>
where
    T: Deserialize<'de>,
{
    type Err = std::io::Error;

    fn from_str(path: &str) -> std::io::Result<Self> {
        let f = std::fs::File::open(path)?;
        let mut deser = serde_json::Deserializer::from_reader(f);
        T::deserialize(&mut deser)
            .map(|value| Self {
                path: path.into(),
                value,
            })
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::Other, e.to_string()))
    }
}

impl<T> Deref for JsonFile<T> {
    type Target = T;

    #[inline]
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}

impl<T> DerefMut for JsonFile<T> {
    #[inline]
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.value
    }
}

impl<T> JsonFile<T> {
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
        impl<T> AsRef<T> for $i<T> {
            #[inline]
            fn as_ref(&self) -> &T {
                self
            }
        }

        impl<T> PartialEq for $i<T>
        where
            T: PartialEq,
        {
            fn eq(&self, rhs: &Self) -> bool {
                self.as_inner() == rhs.as_inner()
            }
        }

        impl<T> PartialEq<T> for $i<T>
        where
            T: PartialEq,
        {
            fn eq(&self, rhs: &T) -> bool {
                self.as_inner() == rhs
            }
        }

        impl<T> Eq for $i<T> where T: Eq {}

        impl<T> PartialOrd for $i<T>
        where
            T: PartialOrd,
        {
            fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
                self.as_inner().partial_cmp(other.as_inner())
            }
        }

        impl<T> PartialOrd<T> for $i<T>
        where
            T: PartialOrd,
        {
            fn partial_cmp(&self, other: &T) -> Option<Ordering> {
                self.as_inner().partial_cmp(other)
            }
        }

        impl<T> Ord for $i<T>
        where
            T: Ord,
        {
            fn cmp(&self, other: &Self) -> Ordering {
                self.as_inner().cmp(other.as_inner())
            }
        }

        impl<T> Hash for $i<T>
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

common_impl!(Json);
common_impl!(JsonFile);

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
