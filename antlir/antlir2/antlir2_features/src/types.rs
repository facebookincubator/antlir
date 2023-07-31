/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::ops::Deref;
use std::path::Path;
use std::path::PathBuf;

use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

macro_rules! path_wrapper {
    ($i:ident, $doc:literal) => {
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
        #[doc = $doc]
        pub struct $i<'a>(Cow<'a, Path>);

        impl<'a> $i<'a> {
            #[inline]
            pub fn path(&self) -> &Path {
                self
            }

            #[inline]
            pub fn into_owned(self) -> PathBuf {
                self.0.into_owned()
            }
        }

        impl<'a> AsRef<Path> for $i<'a> {
            #[inline]
            fn as_ref(&self) -> &Path {
                &self.0
            }
        }

        impl<'a> Deref for $i<'a> {
            type Target = Path;

            #[inline]
            fn deref(&self) -> &Path {
                &self.0
            }
        }

        impl<'a, P> From<P> for $i<'a>
        where
            P: Into<Cow<'a, Path>>,
        {
            fn from(p: P) -> Self {
                Self(p.into())
            }
        }
    };
}

path_wrapper!(BuckOutSource, "A path on the host, populated by Buck");
path_wrapper!(PathInLayer, "A path inside an image layer");

/// Serialized buck2 LayerInfo provider
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct LayerInfo<'a> {
    pub label: Label,
    pub subvol_symlink: Cow<'a, Path>,
    pub depgraph: Cow<'a, Path>,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct UserName<'a>(Cow<'a, str>);

impl<'a> UserName<'a> {
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl<'a, S> From<S> for UserName<'a>
where
    S: Into<Cow<'a, str>>,
{
    fn from(s: S) -> Self {
        Self(s.into())
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct GroupName<'a>(Cow<'a, str>);

impl<'a> GroupName<'a> {
    pub fn name(&self) -> &str {
        &self.0
    }
}

impl<'a, S> From<S> for GroupName<'a>
where
    S: Into<Cow<'a, str>>,
{
    fn from(s: S) -> Self {
        Self(s.into())
    }
}
