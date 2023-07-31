/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::FileType;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
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

impl<'f> antlir2_feature_impl::Feature<'f> for Requires {
    fn provides(&self) -> Result<Vec<Item<'f>>> {
        Ok(Default::default())
    }

    #[deny(unused_variables)]
    fn requires(&self) -> Result<Vec<Requirement<'f>>> {
        let Self {
            files,
            users,
            groups,
        } = self;
        Ok(files
            .iter()
            .map(|p| {
                Requirement::ordered(
                    ItemKey::Path(p.to_owned().into()),
                    Validator::FileType(FileType::File),
                )
            })
            .chain(users.iter().map(|u| {
                Requirement::ordered(ItemKey::User(u.to_owned().into()), Validator::Exists)
            }))
            .chain(groups.iter().map(|g| {
                Requirement::ordered(ItemKey::Group(g.to_owned().into()), Validator::Exists)
            }))
            .collect())
    }

    fn compile(&self, _ctx: &CompilerContext) -> Result<()> {
        // entirely implemented in the depgraph
        Ok(())
    }
}
