/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::os::unix::fs::PermissionsExt;

use antlir2_features::usergroup::Group;
use antlir2_features::usergroup::User;
use antlir2_features::usergroup::UserMod;
use antlir2_users::group::GroupRecord;
use antlir2_users::passwd::UserRecord;
use antlir2_users::Id;
use antlir2_users::Password;
use anyhow::Context;

use crate::CompileFeature;
use crate::CompilerContext;
use crate::Error;
use crate::Result;

trait NextAvailableExt {
    type Id: Id;
    type UsedIds: Iterator<Item = Self::Id>;

    fn used_ids(&self) -> Self::UsedIds;

    fn next_available_id(&self) -> Option<Self::Id> {
        let candidate = self
            .used_ids()
            .map(Self::Id::into_raw)
            // this numbers is semi-magical, but matches the defaults in
            // /etc/login.defs for choosing new ids
            .filter(|&id| id >= 1000 && id <= 60000)
            .max()
            .unwrap_or(999)
            + 1;
        if candidate > 6000 {
            None
        } else {
            Some(Self::Id::from_raw(candidate))
        }
    }
}

impl NextAvailableExt for antlir2_users::passwd::EtcPasswd<'_> {
    type Id = antlir2_users::UserId;
    type UsedIds = <Vec<Self::Id> as IntoIterator>::IntoIter;

    fn used_ids(&self) -> Self::UsedIds {
        self.records()
            .map(|r| r.uid)
            .collect::<Vec<_>>()
            .into_iter()
    }
}

impl NextAvailableExt for antlir2_users::group::EtcGroup<'_> {
    type Id = antlir2_users::GroupId;
    type UsedIds = <Vec<Self::Id> as IntoIterator>::IntoIter;

    fn used_ids(&self) -> Self::UsedIds {
        self.records()
            .map(|r| r.gid)
            .collect::<Vec<_>>()
            .into_iter()
    }
}

impl<'a> CompileFeature for User<'a> {
    #[tracing::instrument(name = "user", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        let mut user_db = ctx.user_db()?;
        let uid = match self.uid {
            Some(uid) => uid.id().into(),
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
                let gid = groups_db
                    .next_available_id()
                    .context("no more gids available")?;
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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use antlir2_users::group::EtcGroup;
    use antlir2_users::passwd::EtcPasswd;
    use antlir2_users::GroupId;
    use antlir2_users::UserId;

    use super::*;

    #[test]
    fn ids_from_correct_range() {
        let passwd = EtcPasswd::parse(
            r#"root:x:0:0:root:/root:/bin/bash
bin:x:1:1:bin:/bin:/sbin/nologin
nobody:x:65534:65534:Kernel Overflow User:/:/sbin/nologin
"#,
        )
        .expect("failed to parse passwd");
        assert_eq!(Some(UserId::from(1000)), passwd.next_available_id());
        let groups = EtcGroup::parse(
            r#"root:x:0:
bin:x:1:root,daemon
nobody:x:65534:
"#,
        )
        .expect("failed to parse group");
        assert_eq!(Some(GroupId::from(1000)), groups.next_available_id());
    }

    #[test]
    fn id_space_exhausted() {
        let mut passwd = EtcPasswd::default();
        let mut group = EtcGroup::default();
        for id in 0..=60000 {
            passwd.push(UserRecord {
                name: format!("u{id}").into(),
                password: Password::Shadow,
                uid: id.into(),
                gid: id.into(),
                comment: "".into(),
                homedir: Path::new("/").into(),
                shell: Path::new("/bin/nologin").into(),
            });
            group.push(GroupRecord {
                name: format!("u{id}").into(),
                password: Password::Shadow,
                gid: id.into(),
                users: Vec::new(),
            });
        }
        assert_eq!(
            passwd.next_available_id(),
            None,
            "id space was exhausted, no uid should be possible"
        );
        assert_eq!(
            group.next_available_id(),
            None,
            "id space was exhausted, no gid should be possible"
        );
    }
}
