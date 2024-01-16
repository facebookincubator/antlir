/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use nom::IResult;

static MAGIC_HEADER: &[u8] = b"btrfs-stream\0";

pub(crate) mod cmd;
mod tlv;

use bytes::BytesMut;
use tokio::io::AsyncRead;
use tokio::io::AsyncReadExt;

#[derive(Debug)]
pub enum ParserControl {
    KeepGoing,
    Enough,
}

/// Parse a chunk of bytes to see if we can extract the header expected atop each sendstream.
fn parse_header<'a>(input: &'a [u8]) -> IResult<&'a [u8], u32> {
    let (remainder, (_magic, version)) = nom::sequence::tuple((
        nom::bytes::streaming::tag::<&[u8], &[u8], nom::error::Error<&[u8]>>(MAGIC_HEADER),
        nom::number::streaming::le_u32,
    ))(input)?;
    Ok((remainder, version))
}

/// Parse an async source of bytes, expecting to find it to contain one or more sendstreams.
/// Because the parsed commands reference data owned by the source, we do not collect the commands.
/// Instead, we allow the caller to process them via `f`, which can instruct the processing to
/// continue or shut down gracefully via the returned `ParserControl`.
///
/// Each sendstream is expected to (1) start with a header, followed by (2) either a Subvol or
/// Snapshot command, followed by (3) 0 or more additional commands, terminated by (4) an End
/// command. Note that we don't validate #2 here, but we do expect #1 and #4.
///
/// Returns number of commands parsed.
///
/// See https://btrfs.readthedocs.io/en/latest/dev/dev-send-stream.html for reference.
pub async fn parse<'a, R, F>(mut reader: R, mut f: F) -> crate::Result<u128>
where
    R: AsyncRead + Unpin + Send,
    F: FnMut(&crate::Command<'_>) -> ParserControl + Send,
{
    let mut unparsed = BytesMut::with_capacity(1000);
    let mut command_count = 0;
    let mut header: Option<u32> = None;
    'read_bytes: loop {
        let bytes_read = reader.read_buf(&mut unparsed).await?;
        if bytes_read != 0 || !unparsed.is_empty() {
            while header.is_some() {
                match crate::Command::parse(&unparsed) {
                    Ok((remainder, command)) => {
                        command_count += 1;
                        if let ParserControl::Enough = f(&command) {
                            // caller got what they needed, no need to continue parsing
                            return Ok(command_count);
                        }
                        if let crate::Command::End = command {
                            unparsed = remainder.into();
                            header = None;
                            continue 'read_bytes;
                        }
                        unparsed = remainder.into();
                    }
                    Err(nom::Err::Error(err)) | Err(nom::Err::Failure(err)) => {
                        Err(crate::Error::Unparsable(format!("{err:?}")))?
                    }
                    Err(nom::Err::Incomplete(_)) => {
                        if bytes_read == 0 {
                            // we've found extra data that cannot be parsed w/nothing more to read
                            Err(crate::Error::TrailingData(unparsed.clone().into()))?
                        }
                        continue 'read_bytes;
                    }
                }
            }
            match parse_header(&unparsed) {
                Ok((remainder, _version)) => {
                    header = Some(_version);
                    unparsed = remainder.into();
                }
                Err(nom::Err::Error(err)) | Err(nom::Err::Failure(err)) => {
                    Err(crate::Error::Unparsable(format!("{err:?}")))?
                }
                Err(nom::Err::Incomplete(_)) => continue,
            };
        } else {
            break 'read_bytes;
        }
    }
    if header.is_some() {
        // We've found the end of the data but not the end of the last sendstream
        Err(crate::Error::Incomplete)?;
    }
    Ok(command_count)
}
