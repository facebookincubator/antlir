/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::hash::Hasher;
use std::path::Path;

use antlir2_mode::Mode;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use serde_with::serde_as;
use serde_with::DisplayFromStr;
use twox_hash::XxHash64;

#[serde_as]
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) struct Entry {
    #[serde_as(as = "DisplayFromStr")]
    pub(crate) mode: Mode,
    #[serde(default)]
    pub(crate) text: Option<String>,
    #[serde(default)]
    pub(crate) content_hash: Option<u64>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) xattrs: BTreeMap<OsString, Vec<u8>>,
}

impl Entry {
    pub fn new(path: &Path) -> Result<Self> {
        let meta = std::fs::metadata(path).context("while statting file")?;
        let mode = Mode::from(meta.permissions());
        let contents = std::fs::read(path).context("while reading file")?;
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(&contents);
        let content_hash = hasher.finish();
        let text = String::from_utf8(contents).ok();
        let xattrs = xattr::list(path)
            .context("while listing xattrs")?
            .filter_map(|name| {
                xattr::get(path, &name)
                    .context("while reading xattr")
                    .map(|value| value.map(|value| (name, value)))
                    .transpose()
            })
            .collect::<Result<_>>()?;
        Ok(Self {
            mode,
            xattrs,
            content_hash: if text.is_none() {
                Some(content_hash)
            } else {
                None
            },
            text,
        })
    }
}
