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
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_features::types::GroupName;
use antlir2_features::types::PathInLayer;
use antlir2_features::types::UserName;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;
use tracing as _;

pub type Feature = Requires;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Requires {
    #[serde(default)]
    pub files: Vec<PathInLayer>,
    #[serde(default)]
    pub users: Vec<UserName>,
    #[serde(default)]
    pub groups: Vec<GroupName>,
}

impl antlir2_depgraph_if::RequiresProvides for Requires {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(Default::default())
    }

    #[deny(unused_variables)]
    fn requires(&self) -> Result<Vec<Requirement>, String> {
        let Self {
            files,
            users,
            groups,
        } = self;
        Ok(files
            .iter()
            .map(|p| {
                Requirement::ordered(
                    ItemKey::Path(p.to_owned()),
                    Validator::FileType(FileType::File),
                )
            })
            .chain(
                users
                    .iter()
                    .map(|u| Requirement::ordered(ItemKey::User(u.to_owned()), Validator::Exists)),
            )
            .chain(
                groups
                    .iter()
                    .map(|g| Requirement::ordered(ItemKey::Group(g.to_owned()), Validator::Exists)),
            )
            .collect())
    }
}

impl antlir2_compile::CompileFeature for Requires {
    fn compile(&self, _ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        // entirely implemented in the depgraph
        Ok(())
    }
}
