/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub mod group;
pub mod passwd;
pub mod shadow;
pub mod uidmaps;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("I/O error: {0}")]
    Io(String),
    #[error("parse error: {0}")]
    Parse(String),
    #[error("{0} defined twice, first as {1} and then as {2}")]
    Duplicate(String, String, String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

pub trait Id: Copy + std::fmt::Debug {
    fn from_raw(id: u32) -> Self
    where
        Self: Sized;

    fn as_raw(&self) -> u32;
    fn into_raw(self) -> u32;
}

macro_rules! id_type {
    ($i:ident, $n:ty) => {
        #[derive(
            Debug,
            Copy,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            derive_more::From,
            derive_more::Into,
            serde::Serialize,
            serde::Deserialize
        )]
        #[repr(transparent)]
        pub struct $i(u32);

        impl crate::Id for $i {
            #[inline]
            fn from_raw(id: u32) -> Self {
                Self(id)
            }

            #[inline]
            fn as_raw(&self) -> u32 {
                self.0
            }

            #[inline]
            fn into_raw(self) -> u32 {
                self.0
            }
        }

        impl From<$i> for $n {
            fn from(i: $i) -> $n {
                <$n>::from_raw(i.as_raw())
            }
        }

        impl std::fmt::Display for $i {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

id_type!(UserId, nix::unistd::Uid);
id_type!(GroupId, nix::unistd::Gid);
