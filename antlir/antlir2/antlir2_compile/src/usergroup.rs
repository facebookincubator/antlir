/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use antlir2_features::usergroup::Group;
use antlir2_features::usergroup::User;
use antlir2_features::usergroup::UserMod;
use antlir2_users::group::GroupRecord;
use antlir2_users::passwd::UserRecord;
use antlir2_users::Password;

use crate::CompileFeature;
use crate::CompilerContext;
use crate::Error;
use crate::Result;

impl<'a> CompileFeature for User<'a> {
    #[tracing::instrument(name = "user", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let mut user_db = ctx.user_db()?;
        let uid = match self.uid {
            Some(uid) => uid.id().into(),
            None => {
                let uid = user_db.next_available_uid();
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
        user_db.push(record);
        std::fs::write(ctx.dst_path("/etc/passwd"), user_db.to_string())?;
        let mut groups_db = ctx.groups_db()?;
        for group in self
            .supplementary_groups
            .iter()
            .chain(vec![&self.primary_group])
        {
            groups_db
                .get_group_by_name_mut(group.name())
                .ok_or_else(|| Error::NoSuchGroup(group.name().to_owned()))?
                .users
                .push(Cow::Borrowed(self.name.name()));
        }
        std::fs::write(ctx.dst_path("/etc/group"), groups_db.to_string())?;
        Ok(())
    }
}

impl<'a> CompileFeature for UserMod<'a> {
    #[tracing::instrument(skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let mut groups_db = ctx.groups_db()?;
        for group in &self.add_supplementary_groups {
            groups_db
                .get_group_by_name_mut(group.name())
                .ok_or_else(|| Error::NoSuchGroup(group.name().to_owned()))?
                .users
                .push(Cow::Borrowed(self.username.name()));
        }
        std::fs::write(ctx.dst_path("/etc/group"), groups_db.to_string())?;
        Ok(())
    }
}

impl<'a> CompileFeature for Group<'a> {
    #[tracing::instrument(skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let mut groups_db = ctx.groups_db()?;
        let gid = match self.gid {
            Some(gid) => gid.id().into(),
            None => {
                let gid = groups_db.next_available_gid();
                tracing::trace!("next available gid = {gid}");
                gid
            }
        };
        let record = GroupRecord {
            name: self.name.name().into(),
            password: Password::Shadow,
            gid,
            users: Vec::new(),
        };
        groups_db.push(record);
        std::fs::write(ctx.dst_path("/etc/group"), groups_db.to_string())?;
        Ok(())
    }
}
