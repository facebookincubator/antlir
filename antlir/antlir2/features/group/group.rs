/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::Group as GroupItem;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_features::types::GroupName;
use antlir2_users::group::GroupRecord;
use antlir2_users::NextAvailableId;
use antlir2_users::Password;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

pub type Feature = Group;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Group {
    pub groupname: GroupName,
    pub gid: Option<u32>,
}

impl antlir2_depgraph::requires_provides::RequiresProvides for Group {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(vec![Item::Group(GroupItem {
            name: self.groupname.to_owned(),
        })])
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(vec![Requirement::ordered(
            ItemKey::Path(std::path::Path::new("/etc/group").into()),
            Validator::Exists,
        )])
    }
}

impl antlir2_compile::CompileFeature for Group {
    #[tracing::instrument(skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let mut groups_db = ctx.groups_db()?;
        let gid = match self.gid {
            Some(gid) => gid.into(),
            None => {
                let gid = groups_db
                    .next_available_id()
                    .context("no more gids available")?;
                tracing::trace!("next available gid = {gid}");
                gid
            }
        };
        let record = GroupRecord {
            name: self.groupname.to_owned().into(),
            password: Password::Shadow,
            gid,
            users: Vec::new(),
        };
        groups_db.push(record);
        std::fs::write(ctx.dst_path("/etc/group")?, groups_db.to_string())?;
        Ok(())
    }
}
