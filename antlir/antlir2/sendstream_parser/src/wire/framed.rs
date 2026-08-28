/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use bytes::Buf;
use bytes::BytesMut;
use nom::IResult;
use nom::Parser;
use tokio_util::codec::Decoder;

use crate::Command;
use crate::Error;
use crate::wire::NomBytes;

pub(super) struct SendstreamDecoder {
    state: State,
}

impl SendstreamDecoder {
    pub(super) fn new() -> Self {
        Self {
            state: State::Empty,
        }
    }
}

enum State {
    /// Nothing has been read yet
    Empty,
    /// Parsing a sendstream of this version
    Parsing(u32),
}

pub(super) enum Item {
    /// Magic header that starts a sendstream - the only data here is the
    /// sendstream version
    SendstreamStart(#[allow(dead_code)] u32),
    Command(Command),
}

static MAGIC_HEADER: &[u8] = b"btrfs-stream\0";

/// Parse a chunk of bytes to see if we can extract the header expected atop each sendstream.
fn sendstream_header(input: NomBytes) -> IResult<NomBytes, u32> {
    let (remainder, (_magic, version)) = (
        nom::bytes::streaming::tag::<&[u8], NomBytes, nom::error::Error<NomBytes>>(MAGIC_HEADER),
        nom::number::streaming::le_u32,
    )
        .parse(input)?;
    Ok((remainder, version))
}

impl Decoder for SendstreamDecoder {
    type Item = Item;
    type Error = Error;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        // TODO: make a NomBytes for BytesMut too? This copy feels bad
        let parsable: NomBytes = src.clone().into();
        let starting_len = parsable.len();
        match self.state {
            State::Empty => match sendstream_header(parsable) {
                Ok((remaining, version)) => {
                    src.advance(starting_len - remaining.len());
                    self.state = State::Parsing(version);
                    Ok(Some(Item::SendstreamStart(version)))
                }
                Err(nom::Err::Incomplete(needed)) => {
                    if let nom::Needed::Size(s) = needed {
                        src.reserve(s.into());
                    }
                    Ok(None)
                }
                Err(e) => Err(Error::Unparsable(e.to_string())),
            },
            State::Parsing(version) => {
                match nom::branch::alt((
                    sendstream_header.map(Item::SendstreamStart),
                    Command::parser(version).map(Item::Command),
                ))
                .parse(parsable)
                {
                    Ok((remaining, item)) => {
                        src.advance(starting_len - remaining.len());
                        if let Item::SendstreamStart(version) = item {
                            self.state = State::Parsing(version);
                        }
                        Ok(Some(item))
                    }
                    Err(nom::Err::Incomplete(needed)) => {
                        if let nom::Needed::Size(s) = needed {
                            src.reserve(s.into());
                        }
                        Ok(None)
                    }
                    Err(e) => Err(Error::Unparsable(e.to_string())),
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use quickcheck::quickcheck;

    use super::*;

    fn headers_survive_arbitrary_chunking(versions: Vec<u32>, chunk_sizes: Vec<u8>) -> bool {
        let versions = versions.into_iter().take(32).collect::<Vec<_>>();
        let encoded = versions
            .iter()
            .flat_map(|version| MAGIC_HEADER.iter().copied().chain(version.to_le_bytes()))
            .collect::<Vec<_>>();
        let chunk_sizes = if chunk_sizes.is_empty() {
            vec![1]
        } else {
            chunk_sizes
        };

        let mut decoder = SendstreamDecoder::new();
        let mut buffer = BytesMut::new();
        let mut decoded = Vec::new();
        let mut offset = 0;
        let mut chunk_index = 0;
        while offset < encoded.len() {
            let chunk_len = 1 + usize::from(chunk_sizes[chunk_index % chunk_sizes.len()] % 23);
            let end = (offset + chunk_len).min(encoded.len());
            buffer.extend_from_slice(&encoded[offset..end]);
            offset = end;
            chunk_index += 1;

            loop {
                match decoder.decode(&mut buffer) {
                    Ok(Some(Item::SendstreamStart(version))) => decoded.push(version),
                    Ok(Some(Item::Command(_))) | Err(_) => return false,
                    Ok(None) => break,
                }
            }
        }

        decoded == versions && buffer.is_empty()
    }

    #[test]
    fn headers_survive_arbitrary_chunking_quickcheck() {
        quickcheck(headers_survive_arbitrary_chunking as fn(Vec<u32>, Vec<u8>) -> bool);
    }
}
