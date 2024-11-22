/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

use serde::Deserialize;

use crate::Error;
use crate::GroupId;
use crate::Id;
use crate::Result;
use crate::UserId;

#[derive(Debug, Deserialize)]
pub struct UidMapUser {
    pub uid: u32,
    pub system: Option<bool>,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UidMapGroup {
    pub gid: u32,
    pub system: Option<bool>,
    pub comment: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct UidMap {
    pub users: BTreeMap<String, UidMapUser>,
    pub groups: BTreeMap<String, UidMapGroup>,
}

impl UidMap {
    pub fn load(path: &Path) -> Result<Self> {
        let file = File::open(path).map_err(|e| Error::Io(e.to_string()))?;
        let reader = BufReader::new(file);
        serde_json::from_reader(reader).map_err(|e| Error::Parse(e.to_string()))
    }

    pub fn get_uid(&self, user: &str) -> Option<UserId> {
        self.users.get(user).map(|u| UserId::from_raw(u.uid))
    }

    pub fn get_gid(&self, group: &str) -> Option<GroupId> {
        self.groups.get(group).map(|g| GroupId::from_raw(g.gid))
    }
}
