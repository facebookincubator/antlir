/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;

use ref_cast::ref_cast_custom;
use ref_cast::RefCastCustom;
use serde::de::Deserializer;
use serde::de::Error as _;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("{0} is not absolute")]
    NotAbsolute(PathBuf),
    #[error("could not canonicalize {0}: {1}")]
    Canonicalize(PathBuf, std::io::Error),
}

impl Error {
    pub fn into_original_path(self) -> PathBuf {
        match self {
            Self::NotAbsolute(p) => p,
            Self::Canonicalize(p, _) => p,
        }
    }
}

pub type Result<R> = std::result::Result<R, Error>;

/// Version of [std::path::PathBuf] that is verified to be an absolute path
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct AbsolutePathBuf(PathBuf);

/// Version of [std::path::Path] that is verified to be an absolute path
#[derive(Debug, PartialEq, Eq, RefCastCustom, Serialize)]
#[serde(transparent)]
#[repr(transparent)]
pub struct AbsolutePath(Path);

impl Deref for AbsolutePathBuf {
    type Target = AbsolutePath;

    fn deref(&self) -> &Self::Target {
        AbsolutePath::new_unchecked(&self.0)
    }
}

impl Deref for AbsolutePath {
    type Target = Path;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}

impl AbsolutePathBuf {
    pub fn new(p: PathBuf) -> Result<Self> {
        match p.is_absolute() {
            true => Ok(Self(p)),
            false => Err(Error::NotAbsolute(p)),
        }
    }

    fn new_unchecked(p: PathBuf) -> Self {
        Self(p)
    }

    /// Attempt to coerce a path into an absolute path, but don't canonicalize
    /// (aka resolve any symlinks) if the path is already absolute.
    pub fn absolutize(p: impl AsRef<Path>) -> Result<Self> {
        if p.as_ref().is_absolute() {
            Ok(Self(p.as_ref().to_path_buf()))
        } else {
            Self::canonicalize(p)
        }
    }

    /// Canonicalize an input path to an absolute path - this resolves any
    /// symlinks and as a result the file pointed to by this path must actually
    /// exist. If you cannot guarantee existence use
    /// [AbsolutePathBuf::absolutize] instead.
    pub fn canonicalize(p: impl AsRef<Path>) -> Result<Self> {
        std::fs::canonicalize(p.as_ref())
            .map(Self)
            .map_err(|e| Error::Canonicalize(p.as_ref().to_path_buf(), e))
    }

    pub fn into_path_buf(self) -> PathBuf {
        self.into()
    }

    pub fn as_absolute_path(&self) -> &AbsolutePath {
        self
    }
}

impl AbsolutePath {
    pub fn new<S: AsRef<OsStr> + ?Sized>(s: &S) -> Result<&Self> {
        let p = Path::new(s);
        match p.is_absolute() {
            true => Ok(Self::new_unchecked(s)),
            false => Err(Error::NotAbsolute(p.to_path_buf())),
        }
    }

    fn new_unchecked<S: AsRef<OsStr> + ?Sized>(s: &S) -> &Self {
        #[ref_cast_custom]
        fn ref_cast(s: &Path) -> &AbsolutePath;

        ref_cast(Path::new(s))
    }

    pub fn join<S: AsRef<Path>>(&self, s: S) -> AbsolutePathBuf {
        AbsolutePathBuf::new_unchecked(self.0.join(s))
    }
}

impl From<&AbsolutePath> for AbsolutePathBuf {
    fn from(abs: &AbsolutePath) -> Self {
        AbsolutePathBuf(abs.0.to_path_buf())
    }
}

impl From<AbsolutePathBuf> for PathBuf {
    fn from(abs: AbsolutePathBuf) -> Self {
        abs.0
    }
}

impl PartialEq<PathBuf> for AbsolutePathBuf {
    fn eq(&self, other: &PathBuf) -> bool {
        self.0 == *other
    }
}

impl PartialEq<PathBuf> for AbsolutePath {
    fn eq(&self, other: &PathBuf) -> bool {
        self.0 == *other
    }
}

impl PartialEq<Path> for AbsolutePathBuf {
    fn eq(&self, other: &Path) -> bool {
        self.0 == *other
    }
}

impl PartialEq<Path> for AbsolutePath {
    fn eq(&self, other: &Path) -> bool {
        self.0 == *other
    }
}

impl TryFrom<PathBuf> for AbsolutePathBuf {
    type Error = Error;

    fn try_from(p: PathBuf) -> Result<Self> {
        Self::new(p)
    }
}

impl<'a> TryFrom<&'a Path> for &'a AbsolutePath {
    type Error = Error;

    fn try_from(p: &'a Path) -> Result<Self> {
        AbsolutePath::new(p)
    }
}

impl AsRef<Path> for AbsolutePath {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl AsRef<Path> for AbsolutePathBuf {
    fn as_ref(&self) -> &Path {
        &self.0
    }
}

impl<'de> Deserialize<'de> for AbsolutePathBuf {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = PathBuf::deserialize(deserializer)?;
        Self::new(path).map_err(D::Error::custom)
    }
}
