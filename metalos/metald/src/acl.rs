/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::marker::PhantomData;

#[derive(Debug, PartialEq)]
pub enum Result<I> {
    Allowed,
    Denied(Vec<Denial<I>>),
}

#[allow(dead_code)] // We'll use this in the next diff
#[derive(Debug, PartialEq)]
pub enum Denial<I> {
    Action {
        accessors: Vec<I>,
        acl_name: String,
        actions: Vec<String>,
    },
    Identity {
        accessors: Vec<I>,
        identity: I,
    },
}

// Generic Permission Checker trait that needs to be implemented for
// Checker to be used by Metald
pub trait PermissionsChecker: Sync + Send {
    type Identity;

    fn check(&self, ids: &[Self::Identity]) -> Result<Self::Identity>;
}

pub struct AllowAll<I>(PhantomData<I>);

impl<I> AllowAll<I> {
    pub fn new() -> Self {
        Self(PhantomData)
    }
}

impl<I> PermissionsChecker for AllowAll<I>
where
    I: Send + Sync,
{
    type Identity = I;
    fn check(&self, _ids: &[Self::Identity]) -> Result<Self::Identity> {
        Result::Allowed
    }
}

impl<C> PermissionsChecker for Box<C>
where
    C: PermissionsChecker + ?Sized,
{
    type Identity = C::Identity;
    fn check(&self, ids: &[Self::Identity]) -> Result<Self::Identity> {
        (**self).check(ids)
    }
}
