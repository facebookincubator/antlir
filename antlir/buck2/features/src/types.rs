/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::ops::Deref;
use std::path::Path;

use buck_label::Label;
use serde::Deserialize;
use serde::Serialize;

/// A buck-built layer target. Currently identified only with the target label,
/// but the location info will be added in a stacked diff.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Layer<'a>(#[serde(borrow)] Label<'a>);

impl<'a> Layer<'a> {
    pub fn label(&self) -> &Label {
        &self.0
    }
}

impl<'a> From<Label<'a>> for Layer<'a> {
    fn from(label: Label<'a>) -> Self {
        Self(label)
    }
}

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
pub struct LayerInfo<'a> {
    pub subvol_symlink: Cow<'a, Path>,
    // antlir2 only
    pub depgraph: Option<Cow<'a, Path>>,
}
