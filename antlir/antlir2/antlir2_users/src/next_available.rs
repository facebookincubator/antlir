/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;

use crate::Id;

pub trait NextAvailableId {
    type Id: Id;
    type UsedIds: Iterator<Item = Self::Id>;

    fn used_ids(&self) -> Self::UsedIds;

    fn next_available_id(&self) -> Option<Self::Id> {
        let used: HashSet<_> = self.used_ids().map(Self::Id::into_raw).collect();
        // these numbers is semi-magical, but matches the defaults in
        // /etc/login.defs for choosing new ids
        for candidate in 1000..60000 {
            if !used.contains(&candidate) {
                return Some(Self::Id::from_raw(candidate));
            }
        }
        None
    }
}

impl NextAvailableId for crate::passwd::EtcPasswd<'_> {
    type Id = crate::UserId;
    type UsedIds = <Vec<Self::Id> as IntoIterator>::IntoIter;

    fn used_ids(&self) -> Self::UsedIds {
        self.records()
            .map(|r| r.uid)
            .collect::<Vec<_>>()
            .into_iter()
    }
}

impl NextAvailableId for crate::group::EtcGroup<'_> {
    type Id = crate::GroupId;
    type UsedIds = <Vec<Self::Id> as IntoIterator>::IntoIter;

    fn used_ids(&self) -> Self::UsedIds {
        self.records()
            .map(|r| r.gid)
            .collect::<Vec<_>>()
            .into_iter()
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::group::EtcGroup;
    use crate::group::GroupRecord;
    use crate::passwd::EtcPasswd;
    use crate::passwd::UserRecord;
    use crate::GroupId;
    use crate::Password;
    use crate::UserId;

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
