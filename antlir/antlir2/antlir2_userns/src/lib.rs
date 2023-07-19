/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::fmt::Display;
use std::ops::Add;
use std::ops::Sub;
use std::str::FromStr;

use serde::Deserialize;
use serde::Serialize;

pub mod ns;
pub mod subid;

pub trait Id:
    Debug
    + Copy
    + From<u32>
    + Into<u32>
    + PartialEq
    + Eq
    + PartialOrd
    + Ord
    + Add<IdOffset, Output = Self>
    + Sub<IdOffset, Output = Self>
    + FromStr<Err = std::num::ParseIntError>
    + Display
{
    fn as_u32(self) -> u32;
}

macro_rules! int_type {
    ($t:ident) => {
        #[derive(
            Debug,
            Copy,
            Clone,
            PartialEq,
            Eq,
            PartialOrd,
            Ord,
            Hash,
            Serialize,
            Deserialize
        )]
        #[serde(transparent)]
        #[repr(transparent)]
        pub struct $t(u32);

        impl From<$t> for u32 {
            fn from(t: $t) -> u32 {
                t.0
            }
        }

        impl From<u32> for $t {
            fn from(u: u32) -> Self {
                Self(u)
            }
        }

        impl FromStr for $t {
            type Err = std::num::ParseIntError;

            fn from_str(s: &str) -> Result<Self, std::num::ParseIntError> {
                s.parse().map(Self)
            }
        }

        impl Display for $t {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                write!(f, "{}", self.0)
            }
        }
    };
}

macro_rules! id_type {
    ($t:ident) => {
        int_type!($t);

        impl Add<IdOffset> for $t {
            type Output = $t;

            fn add(self, offset: IdOffset) -> $t {
                Self::from(self.0 + offset.0)
            }
        }

        impl Sub<IdOffset> for $t {
            type Output = $t;

            fn sub(self, offset: IdOffset) -> $t {
                Self::from(self.0 - offset.0)
            }
        }

        impl Id for $t {
            fn as_u32(self) -> u32 {
                self.0
            }
        }
    };
}

id_type!(Uid);
id_type!(Gid);
int_type!(IdOffset);

impl IdOffset {
    pub fn as_u32(self) -> u32 {
        self.0
    }
}

impl Add for IdOffset {
    type Output = Self;

    fn add(self, rhs: Self) -> Self::Output {
        Self(self.0 + rhs.0)
    }
}

impl Sub for IdOffset {
    type Output = Self;

    fn sub(self, rhs: Self) -> Self::Output {
        Self(self.0 - rhs.0)
    }
}
