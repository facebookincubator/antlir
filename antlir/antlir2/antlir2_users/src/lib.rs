/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Display;
use std::fmt::Formatter;

pub mod group;
pub mod passwd;
pub mod shadow;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("parse error: {0}")]
    Parse(String),
}

pub type Result<T> = std::result::Result<T, Error>;

pub trait Id: Copy + std::fmt::Debug {
    fn from_raw(id: u32) -> Self
    where
        Self: Sized;

    fn as_raw(&self) -> u32;
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

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub enum Password {
    Shadow,
    Locked,
}

impl Password {
    pub(crate) fn parse<'a, E>(input: &'a str) -> nom::IResult<&'a str, Self, E>
    where
        E: nom::error::ParseError<&'a str> + nom::error::ContextError<&'a str>,
    {
        let (input, char) = nom::error::context(
            "password",
            nom::branch::alt((
                nom::character::complete::char('x'),
                nom::character::complete::char('!'),
            )),
        )(input)?;
        Ok((
            input,
            match char {
                'x' => Self::Shadow,
                '!' => Self::Locked,
                _ => unreachable!("parser would have failed"),
            },
        ))
    }
}

impl Display for Password {
    fn fmt(&self, f: &mut Formatter) -> std::fmt::Result {
        match self {
            Self::Shadow => write!(f, "x"),
            Self::Locked => write!(f, "!"),
        }
    }
}
