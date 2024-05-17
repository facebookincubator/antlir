/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::FsEntry;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_features::types::BuckOutSource;
use antlir2_features::types::PathInLayer;
use anyhow as _;
use extract::copy_dep;
use serde::de::Deserializer;
use serde::de::Error as _;
use serde::Deserialize;
use serde::Serialize;
use tracing::trace;

pub type Feature = ExtractBuckBinary;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct ExtractBuckBinary {
    pub src: BuckOutSource,
    pub dst: PathInLayer,
    pub libs: Libs,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Libs {
    #[serde(deserialize_with = "Libs::deserialize_manifest_file")]
    manifest: Manifest,
    libs_dir: PathBuf,
}

impl Libs {
    fn deserialize_manifest_file<'de, D>(deserializer: D) -> Result<Manifest, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = PathBuf::deserialize(deserializer)?;
        let f =
            BufReader::new(File::open(&path).map_err(|e| {
                D::Error::custom(format!("failed to open {}: {e}", path.display()))
            })?);
        serde_json::from_reader(f).map_err(D::Error::custom)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Manifest(pub Vec<Lib>);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Lib {
    pub src_relpath: PathBuf,
    pub dst: LibDstPath,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum LibDstPath {
    Absolute(PathBuf),
    Relative(PathBuf),
}

impl antlir2_depgraph_if::RequiresProvides for ExtractBuckBinary {
    fn provides(&self) -> Result<Vec<Item>, String> {
        // Intentionally provide only the direct files the user asked for,
        // because we don't want to produce conflicts with all the transitive
        // dependencies. However, we will check that any duplicated items are in
        // fact identical, to prevent insane mismatches like this
        // https://fb.workplace.com/groups/btrmeup/posts/5913570682055882
        Ok(vec![Item::Path(PathItem::Entry(FsEntry {
            path: self.dst.to_owned(),
            file_type: FileType::File,
            mode: 0o555,
        }))])
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(vec![Requirement::ordered(
            ItemKey::Path(self.dst.parent().expect("dst always has parent").to_owned()),
            Validator::FileType(FileType::Directory),
        )])
    }
}

impl antlir2_compile::CompileFeature for ExtractBuckBinary {
    #[tracing::instrument(
        name = "extract_buck_binary",
        skip_all,
        fields(src=self.src.display().to_string(), dst=self.dst.display().to_string()),
        ret,
        err,
    )]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let src = self.src.canonicalize()?;
        trace!(
            "copying {} -> {}",
            src.display(),
            ctx.dst_path(&self.dst)?.display()
        );
        // don't copy the metadata from the buck binary, the owner will
        // be wrong
        std::fs::copy(src, ctx.dst_path(&self.dst)?)?;

        for lib in &self.libs.manifest.0 {
            let dst = match &lib.dst {
                LibDstPath::Absolute(a) => Cow::Borrowed(a.as_path()),
                LibDstPath::Relative(r) => {
                    Cow::Owned(self.dst.parent().expect("dst always has parent").join(r))
                }
            };
            trace!("copying {} -> {}", lib.src_relpath.display(), dst.display());
            copy_dep(
                &self.libs.libs_dir.join(&lib.src_relpath),
                &ctx.dst_path(&dst)?,
            )?;
        }
        Ok(())
    }
}
