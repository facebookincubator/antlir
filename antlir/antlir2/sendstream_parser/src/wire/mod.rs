/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use nom::IResult;

use crate::Sendstream;

static MAGIC_HEADER: &[u8] = b"btrfs-stream\0";

pub(crate) mod cmd;
mod tlv;
use crate::Error;
use crate::Result;

impl<'a> Sendstream<'a> {
    fn parse(input: &'a [u8]) -> IResult<&'a [u8], Self> {
        let (input, _) = nom::bytes::complete::tag(MAGIC_HEADER)(input)?;
        let (input, version) = nom::number::complete::le_u32(input)?;
        assert_eq!(1, version);
        let (input, commands) = nom::multi::many1(crate::Command::parse)(input)?;
        Ok((input, Self { commands }))
    }

    pub fn parse_all(input: &'a [u8]) -> Result<Vec<Self>> {
        match nom::combinator::complete(nom::multi::many1(Sendstream::parse))(input) {
            Ok((left, sendstreams)) => {
                if !left.is_empty() {
                    Err(Error::TrailingData(left.to_vec()))
                } else {
                    Ok(sendstreams)
                }
            }
            Err(e) => match e {
                nom::Err::Error(e) | nom::Err::Failure(e) => Err(e.into()),
                nom::Err::Incomplete(_) => Err(Error::Incomplete),
            },
        }
    }
}
