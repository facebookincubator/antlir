// (c) Meta Platforms, Inc. and affiliates. Confidential and proprietary.

use serde::Deserialize;
use serde::Serialize;
use uuid::Uuid;

use super::Fact;
use super::Key;
use crate::fact_impl;

/// Describe an fbpkg that was installed into the image at some point. We don't
/// necessarily know *where* that was installed, just that it was (it could even
/// have been removed!), but that doesn't matter for the purposes of just
/// tracking what fbpkgs went into an image.
/// This is serialized directly by the buck rules for fbpkg_install,
/// fetch_fbpkg_mount, fbpkg_builder and chef_solo.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct FbpkgInstall {
    name: String,
    tag: String,
    uuid: Uuid,
}

#[fact_impl("antlir2_facts::fact::facebook::FbpkgInstall")]
impl Fact for FbpkgInstall {
    fn key(&self) -> Key {
        // a given fbpkg name may be installed multiple times in an image, but
        // the name:tag combo will always have the same uuid, so just use
        // name:tag as the unique identifier that then tells us enough info
        // about what package was installed
        format!("{}:{}", self.name, self.tag).into()
    }
}

impl FbpkgInstall {
    pub fn key(name: &str, tag: &str) -> Key {
        format!("{}:{}", name, tag).into()
    }
}
