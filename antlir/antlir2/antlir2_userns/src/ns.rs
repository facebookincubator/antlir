/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Write;
use std::str::FromStr;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use serde::Deserialize;
use serde::Serialize;

use crate::Id;
use crate::IdOffset;

macro_rules! id_wrapper {
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
            Deserialize,
            Serialize
        )]
        #[serde(transparent)]
        #[repr(transparent)]
        pub struct $t<T>(T)
        where
            T: Id;

        impl<T> $t<T>
        where
            T: Id,
        {
            fn as_id(self) -> T {
                self.0
            }
        }

        impl<T> FromStr for $t<T>
        where
            T: Id,
        {
            type Err = <T as FromStr>::Err;

            fn from_str(s: &str) -> std::result::Result<Self, Self::Err> {
                s.parse().map(Self)
            }
        }

        impl<T> From<T> for $t<T>
        where
            T: Id,
        {
            fn from(t: T) -> Self {
                Self(t)
            }
        }
    };
}

id_wrapper!(Inner);
id_wrapper!(Outer);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NsMapping<T>
where
    T: Id,
{
    ranges: Vec<Range<T>>,
}

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
pub struct Range<T>
where
    T: Id,
{
    inner: Inner<T>,
    outer: Outer<T>,
    len: IdOffset,
}

impl<T> Range<T>
where
    T: Id,
{
    fn to_outer(&self, inner: Inner<T>) -> Option<Outer<T>> {
        let max = self.inner.as_id() + self.len;
        if inner.as_id() >= self.inner.as_id() && inner.as_id() < max {
            let offset = IdOffset(inner.as_id().as_u32() - self.inner.as_id().as_u32());
            Some(Outer(self.outer.as_id() + offset))
        } else {
            None
        }
    }

    fn to_inner(&self, outer: Outer<T>) -> Option<Inner<T>> {
        let max = self.outer.as_id() + self.len;
        if outer.as_id() >= self.outer.as_id() && outer.as_id() < max {
            let offset = IdOffset(outer.as_id().as_u32() - self.outer.as_id().as_u32());
            Some(Inner(self.inner.as_id() + offset))
        } else {
            None
        }
    }
}

impl<T> FromStr for Range<T>
where
    T: Id,
{
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let pieces: Vec<_> = s.split_whitespace().collect();
        let [inner, outer, len]: [&str; 3] = pieces
            .try_into()
            .map_err(|_| anyhow!("expected exactly 3 whitespace separated fields"))?;
        Ok(Self {
            inner: inner
                .parse()
                .with_context(|| format!("bad inner id {inner}"))?,
            outer: outer
                .parse()
                .with_context(|| format!("bad outer id {outer}"))?,
            len: len.parse().with_context(|| format!("bad len {len}"))?,
        })
    }
}

impl<T> FromStr for NsMapping<T>
where
    T: Id,
{
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let ranges = s
            .lines()
            .map(|line| Range::from_str(line).with_context(|| format!("while parsing '{line}'")))
            .collect::<Result<_>>()?;
        Ok(Self { ranges })
    }
}

impl<T> NsMapping<T>
where
    T: Id,
{
    pub fn to_outer(&self, inner: Inner<T>) -> Option<Outer<T>> {
        self.ranges.iter().filter_map(|r| r.to_outer(inner)).next()
    }

    pub fn to_inner(&self, outer: Outer<T>) -> Option<Inner<T>> {
        self.ranges.iter().filter_map(|r| r.to_inner(outer)).next()
    }

    pub fn to_proc_map(&self) -> String {
        let mut s = String::new();
        for range in &self.ranges {
            writeln!(
                s,
                "{} {} {}",
                range.inner.as_id(),
                range.outer.as_id(),
                range.len
            )
            .expect("infallible");
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use rstest::rstest;

    use super::*;
    use crate::Uid;

    #[test]
    fn parse() {
        let map = "0 100 1000\n5000 10000 42"
            .parse::<NsMapping<Uid>>()
            .expect("failed to parse");
        assert_eq!(
            NsMapping {
                ranges: vec![
                    Range {
                        inner: Inner(0.into()),
                        outer: Outer(100.into()),
                        len: 1000.into(),
                    },
                    Range {
                        inner: Inner(5000.into()),
                        outer: Outer(10000.into()),
                        len: 42.into(),
                    },
                ]
            },
            map
        );
        assert_eq!(
            map,
            map.to_proc_map()
                .parse::<NsMapping<Uid>>()
                .expect("failed to parse regenerated")
        );
    }

    #[rstest]
    #[case(0, Some(100))]
    #[case(999, Some(1099))]
    #[case(1000, None)]
    fn range_in_to_out(#[case] inner: u32, #[case] outer: Option<u32>) {
        assert_eq!(
            outer.map(Uid::from).map(Outer),
            Range {
                inner: Inner(0.into()),
                outer: Outer(100.into()),
                len: 1000.into(),
            }
            .to_outer(Inner(Uid::from(inner)))
        );
    }

    #[rstest]
    #[case(100, Some(0))]
    #[case(199, Some(99))]
    #[case(1099, Some(999))]
    #[case(1100, None)]
    fn range_out_to_in(#[case] outer: u32, #[case] inner: Option<u32>) {
        assert_eq!(
            inner.map(Uid::from).map(Inner),
            Range {
                inner: Inner(0.into()),
                outer: Outer(100.into()),
                len: 1000.into(),
            }
            .to_inner(Outer(Uid::from(outer)))
        );
    }

    #[rstest]
    #[case(0, Some(100))]
    #[case(999, Some(1099))]
    #[case(1000, None)]
    #[case(5000, Some(10000))]
    #[case(5001, Some(10001))]
    #[case(5041, Some(10041))]
    #[case(5042, None)]
    fn map_in_to_out(#[case] inner: u32, #[case] outer: Option<u32>) {
        let map: NsMapping<Uid> = NsMapping {
            ranges: vec![
                Range {
                    inner: Inner(0.into()),
                    outer: Outer(100.into()),
                    len: 1000.into(),
                },
                Range {
                    inner: Inner(5000.into()),
                    outer: Outer(10000.into()),
                    len: 42.into(),
                },
            ],
        };
        assert_eq!(
            outer.map(Uid::from).map(Outer),
            map.to_outer(Inner(Uid::from(inner)))
        );
    }

    #[rstest]
    #[case(100, Some(0))]
    #[case(199, Some(99))]
    #[case(1099, Some(999))]
    #[case(1100, None)]
    #[case(5000, None)]
    #[case(10000, Some(5000))]
    #[case(10041, Some(5041))]
    #[case(10042, None)]
    fn map_out_to_in(#[case] outer: u32, #[case] inner: Option<u32>) {
        let map: NsMapping<Uid> = NsMapping {
            ranges: vec![
                Range {
                    inner: Inner(0.into()),
                    outer: Outer(100.into()),
                    len: 1000.into(),
                },
                Range {
                    inner: Inner(5000.into()),
                    outer: Outer(10000.into()),
                    len: 42.into(),
                },
            ],
        };
        assert_eq!(
            inner.map(Uid::from).map(Inner),
            map.to_inner(Outer(Uid::from(outer)))
        );
    }
}
