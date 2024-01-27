/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;

use serde::Deserialize;
use serde::Serialize;

use super::Fact;

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
pub struct User<'a> {
    name: Cow<'a, str>,
    id: u32,
}

impl<'a> Fact<'a, '_> for User<'a> {
    type Key = &'a str;

    fn key(&'a self) -> Self::Key {
        &self.name
    }
}

impl<'a> User<'a> {
    pub fn key<'k>(name: &'k str) -> <User as Fact>::Key {
        name
    }

    pub fn new<N>(name: N, id: u32) -> Self
    where
        N: Into<Cow<'a, str>>,
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
pub struct Group<'a> {
    name: Cow<'a, str>,
    id: u32,
    members: Vec<Cow<'a, str>>,
}

impl<'a> Fact<'a, '_> for Group<'a> {
    type Key = &'a str;

    fn key(&'a self) -> Self::Key {
        &self.name
    }
}

impl<'a> Group<'a> {
    pub fn key<'k>(name: &'k str) -> <Group as Fact>::Key {
        name
    }

    pub fn new<N, I, M>(name: N, id: u32, members: I) -> Self
    where
        N: Into<Cow<'a, str>>,
        I: IntoIterator<Item = M>,
        M: Into<Cow<'a, str>>,
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
