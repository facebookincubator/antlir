/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use antlir2_compile::Arch;
use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::FileType;
use antlir2_depgraph::item::FsEntry;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::item::Path as PathItem;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_features::types::BuckOutSource;
use antlir2_features::types::PathInLayer;
use anyhow as _;
use extract::copy_dep;
use extract::so_dependencies;
use serde::Deserialize;
use serde::Serialize;
use tracing::trace;

pub type Feature = ExtractBuckBinary;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct ExtractBuckBinary {
    pub src: BuckOutSource,
    pub dst: PathInLayer,
}

impl antlir2_depgraph::requires_provides::RequiresProvides for ExtractBuckBinary {
    fn provides(&self) -> Result<Vec<Item<'static>>, String> {
        // Intentionally provide only the direct files the user asked for,
        // because we don't want to produce conflicts with all the transitive
        // dependencies. However, we will check that any duplicated items are in
        // fact identical, to prevent insane mismatches like this
        // https://fb.workplace.com/groups/btrmeup/posts/5913570682055882
        Ok(vec![Item::Path(PathItem::Entry(FsEntry {
            path: self.dst.to_owned().into(),
            file_type: FileType::File,
            mode: 0o555,
        }))])
    }

    fn requires(&self) -> Result<Vec<Requirement<'static>>, String> {
        Ok(vec![Requirement::ordered(
            ItemKey::Path(
                self.dst
                    .parent()
                    .expect("dst always has parent")
                    .to_owned()
                    .into(),
            ),
            Validator::FileType(FileType::Directory),
        )])
    }
}

impl antlir2_compile::CompileFeature for ExtractBuckBinary {
    #[tracing::instrument(name = "extract_buck_binary", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let default_interpreter = Path::new(match ctx.target_arch() {
            Arch::X86_64 => "/usr/lib64/ld-linux-x86-64.so.2",
            Arch::Aarch64 => "/lib/ld-linux-aarch64.so.1",
        });
        let src = self.src.canonicalize()?;
        let deps = so_dependencies(self.src.to_owned(), None, default_interpreter)?;
        for dep in &deps {
            if let Ok(relpath) = dep.strip_prefix(src.parent().expect("src always has parent")) {
                tracing::debug!(
                    relpath = relpath.display().to_string(),
                    "installing library at path relative to dst"
                );
                copy_dep(
                    dep,
                    &ctx.dst_path(
                        &self
                            .dst
                            .parent()
                            .expect("dst always has parent")
                            .join(relpath),
                    )?,
                )?;
            } else {
                copy_dep(dep, &ctx.dst_path(dep.strip_prefix("/").unwrap_or(dep))?)?;
            }
        }
        // don't copy the metadata from the buck binary, the owner will
        // be wrong
        trace!(
            "copying {} -> {}",
            self.src.display(),
            ctx.dst_path(&self.dst)?.display()
        );
        std::fs::copy(&self.src, ctx.dst_path(&self.dst)?)?;
        Ok(())
    }
}
