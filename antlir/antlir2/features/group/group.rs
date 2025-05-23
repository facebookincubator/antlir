/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_depgraph_if::item::Group as GroupItem;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_features::types::BuckOutSource;
use antlir2_features::types::GroupName;
use antlir2_users::GroupId;
use antlir2_users::group::GroupRecord;
use antlir2_users::group::GroupRecordPassword;
use antlir2_users::uidmaps::UidMap;
use anyhow::Context;
use anyhow::anyhow;
use serde::Deserialize;
use serde::Serialize;
use tracing::debug;

pub type Feature = Group;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Group {
    pub groupname: GroupName,
    pub uidmap: BuckOutSource,
}

impl antlir2_depgraph_if::RequiresProvides for Group {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(vec![Item::Group(GroupItem {
            name: self.groupname.to_owned(),
        })])
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        get_gid(&self.uidmap, &self.groupname).map_err(|e| format!("{e:#}"))?;
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
        let new_record = GroupRecord {
            name: self.groupname.to_owned().into(),
            password: GroupRecordPassword::X,
            gid: get_gid(&self.uidmap, &self.groupname)?,
            users: Vec::new(),
        };
        if let Some(existing) = groups_db.get_group_by_name(&self.groupname) {
            debug!(
                "group '{}' already existed and all important fields are identical, not duplicating it",
                self.groupname
            );
            if existing.gid != new_record.gid || existing.password != new_record.password {
                return Err(anyhow!("group '{}' already existed, but has incompatible settings with the new entry - new: {new_record:?}\nold: {existing:?}", self.groupname).into());
            }
            return Ok(());
        }
        groups_db.push(new_record)?;
        std::fs::write(ctx.dst_path("/etc/group")?, groups_db.to_string())?;
        Ok(())
    }
}

fn get_gid(uidmap: &BuckOutSource, groupname: &GroupName) -> anyhow::Result<GroupId> {
    let uidmap = UidMap::load(uidmap)?;
    uidmap
        .get_gid(groupname)
        .with_context(|| format!("group {} not found in uidmap", groupname))
}
