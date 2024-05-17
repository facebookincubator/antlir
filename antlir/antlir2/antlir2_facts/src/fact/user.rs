/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use serde::Deserialize;
use serde::Serialize;

use super::Fact;
use super::Key;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct User {
    name: String,
    id: u32,
}

impl Fact for User {
    fn key(&self) -> Key {
        self.name.as_str().into()
    }
}

impl User {
    pub fn key(name: &str) -> Key {
        name.into()
    }

    pub fn new<N>(name: N, id: u32) -> Self
    where
        N: Into<String>,
    {
        Self {
            name: name.into(),
            id,
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> u32 {
        self.id
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct Group {
    name: String,
    id: u32,
    members: Vec<String>,
}

impl Fact for Group {
    fn key(&self) -> Key {
        self.name.as_str().into()
    }
}

impl Group {
    pub fn key(name: &str) -> Key {
        name.into()
    }

    pub fn new<N, I, M>(name: N, id: u32, members: I) -> Self
    where
        N: Into<String>,
        I: IntoIterator<Item = M>,
        M: Into<String>,
    {
        Self {
            name: name.into(),
            id,
            members: members.into_iter().map(|m| m.into()).collect(),
        }
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn id(&self) -> u32 {
        self.id
    }

    pub fn members(&self) -> impl Iterator<Item = &str> {
        self.members.iter().map(|m| m.as_ref())
    }
}
