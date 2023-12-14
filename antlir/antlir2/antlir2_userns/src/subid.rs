/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
pub use anyhow::Error;
use anyhow::Result;

use crate::Id;
use crate::IdOffset;
use crate::Uid;

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum User {
    Id(Uid),
    Name(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SubMapping<T>
where
    T: Id,
{
    user: User,
    range: Range<T>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Range<T>
where
    T: Id,
{
    start: T,
    len: IdOffset,
}

impl<T> FromStr for SubMapping<T>
where
    T: Id,
{
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let pieces: Vec<_> = s.split(':').collect();
        let [user, start, len]: [&str; 3] = pieces
            .try_into()
            .map_err(|_| anyhow!("expected exactly 3 : separated fields"))?;
        let user = match user.parse::<u32>() {
            Ok(id) => User::Id(id.into()),
            Err(_) => User::Name(user.to_owned()),
        };
        Ok(Self {
            user,
            range: Range {
                start: start.parse().context("while parsing start")?,
                len: len.parse().context("while parsing len")?,
            },
        })
    }
}

impl<T> Range<T>
where
    T: Id,
{
    pub fn start(&self) -> T {
        self.start
    }

    pub fn len(&self) -> IdOffset {
        self.len
    }

    pub fn identity() -> Self {
        Self {
            start: 0.into(),
            len: 65536.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdMap<T>
where
    T: Id,
{
    users: HashMap<User, Vec<Range<T>>>,
}

impl<T> FromStr for IdMap<T>
where
    T: Id,
{
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let mut users: HashMap<User, Vec<Range<T>>> = HashMap::new();
        for line in s.lines() {
            let map = line
                .parse::<SubMapping<T>>()
                .with_context(|| format!("while parsing '{line}'"))?;
            users.entry(map.user).or_default().push(map.range);
        }
        Ok(Self { users })
    }
}

impl<T> IdMap<T>
where
    T: Id,
{
    pub fn ranges(&self, user: &User) -> Option<&[Range<T>]> {
        self.users.get(user).map(Vec::as_slice)
    }

    /// Find a good range to use that will give a full range of 32 bit ids
    pub fn best(&self, user: &User) -> Option<Range<T>> {
        match self.ranges(user) {
            Some(ranges) => ranges
                .iter()
                .find(|range| range.len.as_u32() == 65536)
                .copied(),
            None => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_line() {
        assert_eq!(
            SubMapping::<Uid> {
                user: User::Id(42.into()),
                range: Range {
                    start: 10000.into(),
                    len: 65536.into(),
                }
            },
            "42:10000:65536".parse().expect("failed to parse")
        );
        assert_eq!(
            SubMapping::<Uid> {
                user: User::Name("foo".to_owned()),
                range: Range {
                    start: 10000.into(),
                    len: 65536.into(),
                }
            },
            "foo:10000:65536".parse().expect("failed to parse")
        );
    }

    #[test]
    fn parse_file() {
        assert_eq!(
            IdMap::<Uid> {
                users: HashMap::from([
                    (
                        User::Name("foo".to_owned()),
                        vec![
                            Range {
                                start: 100000.into(),
                                len: 65536.into(),
                            },
                            Range {
                                start: 300000.into(),
                                len: 65536.into(),
                            }
                        ],
                    ),
                    (
                        User::Name("bar".to_owned()),
                        vec![Range {
                            start: 200000.into(),
                            len: 65536.into(),
                        }],
                    ),
                ])
            },
            "foo:100000:65536\nbar:200000:65536\nfoo:300000:65536"
                .parse()
                .expect("failed to parse")
        );
    }

    #[test]
    fn lookup() {
        assert_eq!(
            IdMap::<Uid> {
                users: HashMap::from([
                    (
                        User::Name("foo".to_owned()),
                        vec![
                            Range {
                                start: 100000.into(),
                                len: 65536.into(),
                            },
                            Range {
                                start: 300000.into(),
                                len: 65536.into(),
                            },
                        ],
                    ),
                    (
                        User::Name("bar".to_owned()),
                        vec![Range {
                            start: 200000.into(),
                            len: 65536.into(),
                        }],
                    ),
                ]),
            }
            .ranges(&User::Name("foo".to_owned())),
            Some(
                &[
                    Range {
                        start: 100000.into(),
                        len: 65536.into(),
                    },
                    Range {
                        start: 300000.into(),
                        len: 65536.into(),
                    },
                ][..]
            )
        );
    }

    #[test]
    fn best() {
        assert_eq!(
            IdMap::<Uid> {
                users: HashMap::from([(
                    User::Name("foo".to_owned()),
                    vec![
                        Range {
                            start: 100000.into(),
                            len: 100.into(),
                        },
                        Range {
                            start: 300000.into(),
                            len: 65536.into(),
                        },
                    ],
                ),]),
            }
            .best(&User::Name("foo".to_owned())),
            Some(Range {
                start: 300000.into(),
                len: 65536.into(),
            },)
        );
    }
}
