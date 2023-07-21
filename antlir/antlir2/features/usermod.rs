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

pub type Feature = UserMod<'static>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct UserMod<'a> {
    pub username: UserName<'a>,
    pub add_supplementary_groups: Vec<GroupName<'a>>,
}

impl<'f> antlir2_feature_impl::Feature<'f> for UserMod<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>> {
        Ok(Default::default())
    }

    fn requires(&self) -> Result<Vec<Requirement<'f>>> {
        let mut v = vec![Requirement::ordered(
            ItemKey::User(self.username.name().to_owned().into()),
            Validator::Exists,
        )];
        v.extend(self.add_supplementary_groups.iter().map(|g| {
            Requirement::ordered(
                ItemKey::Group(g.name().to_owned().into()),
                Validator::Exists,
            )
        }));
        Ok(v)
    }

    #[tracing::instrument(skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let mut groups_db = ctx.groups_db()?;
        for group in &self.add_supplementary_groups {
            groups_db
                .get_group_by_name_mut(group.name())
                .with_context(|| format!("no such group '{}'", group.name()))?
                .users
                .push(Cow::Borrowed(self.username.name()));
        }
        std::fs::write(ctx.dst_path("/etc/group"), groups_db.to_string())?;
        Ok(())
    }
}
