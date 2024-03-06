/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_features::types::GroupName;
use antlir2_features::types::UserName;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

pub type Feature = UserMod;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct UserMod {
    pub username: UserName,
    pub add_supplementary_groups: Vec<GroupName>,
}

impl antlir2_depgraph::requires_provides::RequiresProvides for UserMod {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        let mut v = vec![Requirement::ordered(
            ItemKey::User(self.username.to_owned().into()),
            Validator::Exists,
        )];
        v.extend(
            self.add_supplementary_groups.iter().map(|g| {
                Requirement::ordered(ItemKey::Group(g.to_owned().into()), Validator::Exists)
            }),
        );
        Ok(v)
    }
}

impl antlir2_compile::CompileFeature for UserMod {
    #[tracing::instrument(skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let mut groups_db = ctx.groups_db()?;
        for group in &self.add_supplementary_groups {
            groups_db
                .get_group_by_name_mut(group)
                .with_context(|| format!("no such group '{}'", group))?
                .users
                .push(Cow::Borrowed(&self.username));
        }
        std::fs::write(ctx.dst_path("/etc/group")?, groups_db.to_string())?;
        Ok(())
    }
}
