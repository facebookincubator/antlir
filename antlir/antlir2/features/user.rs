/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::os::unix::fs::PermissionsExt;

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::FileType;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::item::User as UserItem;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_features::types::GroupName;
use antlir2_features::types::PathInLayer;
use antlir2_features::types::UserName;
use antlir2_users::passwd::UserRecord;
use antlir2_users::NextAvailableId;
use antlir2_users::Password;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

pub type Feature = User<'static>;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(bound(deserialize = "'de: 'a"))]
pub struct User<'a> {
    pub name: UserName<'a>,
    pub uid: Option<u32>,
    pub primary_group: GroupName<'a>,
    pub supplementary_groups: Vec<GroupName<'a>>,
    pub home_dir: PathInLayer<'a>,
    pub shell: PathInLayer<'a>,
    pub comment: Option<Cow<'a, str>>,
}

impl<'f> antlir2_feature_impl::Feature<'f> for User<'f> {
    fn provides(&self) -> Result<Vec<Item<'f>>> {
        Ok(vec![Item::User(UserItem {
            name: self.name.name().to_owned().into(),
        })])
    }

    fn requires(&self) -> Result<Vec<Requirement<'f>>> {
        let mut v = vec![
            Requirement::unordered(
                ItemKey::Path(self.home_dir.path().to_owned().into()),
                Validator::FileType(FileType::Directory),
            ),
            Requirement::unordered(
                ItemKey::Path(self.shell.path().to_owned().into()),
                Validator::Executable,
            ),
            Requirement::ordered(
                ItemKey::Path(std::path::Path::new("/etc/passwd").into()),
                Validator::Exists,
            ),
            Requirement::ordered(
                ItemKey::Path(std::path::Path::new("/etc/group").into()),
                Validator::Exists,
            ),
        ];
        v.extend(
            self.supplementary_groups
                .iter()
                .chain(vec![&self.primary_group])
                .map(|g| {
                    Requirement::ordered(
                        ItemKey::Group(g.name().to_owned().into()),
                        Validator::Exists,
                    )
                }),
        );
        Ok(v)
    }

    #[tracing::instrument(name = "user", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let mut user_db = ctx.user_db()?;
        let uid = match self.uid {
            Some(uid) => uid.into(),
            None => {
                let uid = user_db
                    .next_available_id()
                    .context("no more uids available")?;
                tracing::trace!("next available uid = {uid}");
                uid
            }
        };
        let record = UserRecord {
            name: self.name.name().into(),
            password: Password::Shadow,
            uid,
            gid: ctx.gid(self.primary_group.name())?,
            comment: self.comment.clone().unwrap_or(Cow::Borrowed("")),
            homedir: self.home_dir.path().to_owned().into(),
            shell: self.shell.path().to_owned().into(),
        };
        let mut shadow_db = ctx.shadow_db()?;
        shadow_db.push(record.new_shadow_record());
        user_db.push(record);
        std::fs::write(ctx.dst_path("/etc/passwd"), user_db.to_string())?;
        std::fs::write(ctx.dst_path("/etc/shadow"), shadow_db.to_string())?;
        std::fs::set_permissions(
            ctx.dst_path("/etc/shadow"),
            std::fs::Permissions::from_mode(0o000),
        )?;

        let mut groups_db = ctx.groups_db()?;
        for group in self
            .supplementary_groups
            .iter()
            .chain(vec![&self.primary_group])
        {
            groups_db
                .get_group_by_name_mut(group.name())
                .with_context(|| format!("no such group '{}'", group.name()))?
                .users
                .push(Cow::Borrowed(self.name.name()));
        }
        std::fs::write(ctx.dst_path("/etc/group"), groups_db.to_string())?;
        Ok(())
    }
}
