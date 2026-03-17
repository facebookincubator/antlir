/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use tracing::debug;
use walkdir::WalkDir;

use super::Out;
use super::storage::Storage;

#[derive(Parser, Debug)]
pub(crate) struct Metadata {
    #[clap(long)]
    tree: PathBuf,
}

impl Metadata {
    #[tracing::instrument(skip(self, storage), ret, err)]
    pub(crate) async fn run(&self, storage: Box<dyn Storage>) -> Result<Out> {
        let mut files = BTreeMap::new();
        let mut checksums = BTreeMap::new();
        debug!("walking {self:?}");

        for entry in WalkDir::new(&self.tree).follow_links(true) {
            let entry = entry.context("failed to walk metadata tree")?;
            debug!("processing {entry:?}");
            if !entry.file_type().is_file() {
                debug!("skipping because file type is {:?}", entry.file_type());
                continue;
            }
            let relative = entry
                .path()
                .strip_prefix(&self.tree)
                .context("failed to compute relative path")?;
            let key = relative.to_str().context("non-utf8 path")?;
            let result = storage.store(entry.path(), key).await?;
            let url = result.url.to_string();
            checksums.insert(url.clone(), result.checksums);
            files.insert(key.to_owned(), url);
        }

        Ok(Out { files, checksums })
    }
}
