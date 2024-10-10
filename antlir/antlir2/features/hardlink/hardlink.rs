/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_features::types::PathInLayer;
use serde::Deserialize;
use serde::Serialize;

pub type Feature = Hardlink;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Hardlink {
    link: PathInLayer,
    target: PathInLayer,
}

impl antlir2_depgraph_if::RequiresProvides for Hardlink {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(vec![Item::Path(Path::Symlink {
            link: self.link.to_owned(),
            target: self.target.to_owned(),
        })])
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(vec![
            Requirement::ordered(
                ItemKey::Path(
                    self.link
                        .parent()
                        .unwrap_or_else(|| std::path::Path::new("/"))
                        .to_owned(),
                ),
                Validator::FileType(FileType::Directory),
            ),
            Requirement::ordered(
                ItemKey::Path(self.target.to_owned()),
                Validator::FileType(FileType::File),
            ),
        ])
    }
}

impl antlir2_compile::CompileFeature for Hardlink {
    #[tracing::instrument(name = "hardlink", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let link = ctx.dst_path(&self.link)?;
        let target = ctx.dst_path(&self.target)?;
        std::fs::hard_link(target, link)?;
        Ok(())
    }
}
