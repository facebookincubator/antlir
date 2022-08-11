/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::fmt::Formatter;
use std::marker::PhantomData;

pub enum Result<I> {
    Allowed,
    Denied(Denied<I>),
}

pub struct Denied<I> {
    pub identities: Vec<I>,
    pub acl_name: String,
    pub action: String,
}

impl<I> Debug for Result<I>
where
    I: Debug,
{
    fn fmt(&self, fmt: &mut Formatter) -> std::fmt::Result {
        match self {
            Self::Allowed => fmt.debug_tuple("Allowed").finish(),
            Self::Denied(d) => Debug::fmt(d, fmt),
        }
    }
}

impl<I> Debug for Denied<I>
where
    I: Debug,
{
    fn fmt(&self, fmt: &mut Formatter) -> std::fmt::Result {
        fmt.debug_struct("Denied")
            .field("identities", &self.identities)
            .field("acl_name", &self.acl_name)
            .field("action", &self.action)
            .finish()
    }
}

impl<I> PartialEq for Result<I>
where
    Denied<I>: PartialEq,
{
    fn eq(&self, rhs: &Self) -> bool {
        match (self, rhs) {
            (Self::Allowed, Self::Allowed) => true,
            (Self::Denied(s), Self::Denied(r)) => s == r,
            _ => false,
        }
    }
}

impl<I> PartialEq for Denied<I>
where
    I: PartialEq,
{
    #[deny(unused_variables)]
    fn eq(&self, rhs: &Self) -> bool {
        let Self {
            identities,
            acl_name,
            action,
        } = self;
        (identities == &rhs.identities) && (acl_name == &rhs.acl_name) && (action == &rhs.action)
    }
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
