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
    Denied(Denied<I>),
}

#[derive(Debug, PartialEq)]
pub struct Denied<I> {
    pub identities: Vec<I>,
    pub acl_name: String,
    pub action: String,
}

// Generic Permission Checker trait that needs to be implemented for
// Checker to be used by Metald
pub trait PermissionsChecker: Sync + Send {
    type Identity;

    fn action_allowed_for_identity(
        &self,
        ids: &[Self::Identity],
        acl_action: &str,
    ) -> Result<Self::Identity>;
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
    fn action_allowed_for_identity(
        &self,
        _ids: &[Self::Identity],
        _action: &str,
    ) -> Result<Self::Identity> {
        Result::Allowed
    }
}

impl<C> PermissionsChecker for Box<C>
where
    C: PermissionsChecker + ?Sized,
{
    type Identity = C::Identity;
    fn action_allowed_for_identity(
        &self,
        ids: &[Self::Identity],
        acl_action: &str,
    ) -> Result<Self::Identity> {
        (**self).action_allowed_for_identity(ids, acl_action)
    }
}
