/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use futures::Stream;
use futures::StreamExt;
use futures::future;
use tokio::io::AsyncRead;
use tokio_util::codec::FramedRead;

pub(crate) mod cmd;
mod framed;
mod nombytes;
mod tlv;
pub use nombytes::NomBytes;

/// Parse an async source of bytes, expecting to find it to contain one or more sendstreams.
/// Because the parsed commands reference data owned by the source, we do not collect the commands.
/// Instead, we allow the caller to process them via `f`, which can instruct the processing to
/// continue or shut down gracefully via the returned `ParserControl`.
///
/// Each sendstream is expected to (1) start with a header, followed by (2) either a Subvol or
/// Snapshot command, followed by (3) 0 or more additional commands, terminated by (4) an End
/// command. Note that only (1) is actually enforced, afterward every command
/// will be emitted into the stream as long as it could be read and parsed.
///
/// See https://btrfs.readthedocs.io/en/latest/dev/dev-send-stream.html for reference.
pub fn parse<R>(reader: R) -> impl Stream<Item = crate::Result<crate::Command>>
where
    R: AsyncRead,
{
    let reader = FramedRead::new(reader, framed::SendstreamDecoder::new());
    reader.filter_map(|item_res| {
        future::ready(match item_res {
            Ok(framed::Item::Command(command)) => Some(Ok(command)),
            Ok(framed::Item::SendstreamStart(_)) => None,
            Err(e) => Some(Err(e)),
        })
    })
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use std::io::Cursor;
    use std::time::Duration;

    use futures::StreamExt;
    use tokio::io::AsyncWriteExt;
    use tokio::time::sleep;

    use super::*;

    /// Historically, we couldn't stream commands as they were parsed very well,
    /// so the early exit was implemented with a not-very-Rusty callback
    /// interface. This test proves that the parser stops operating as soon as
    /// the caller stops asking for commands (via a reader that will only
    /// provide enough bytes for a few commands before then stalling forever)
    #[tokio::test]
    async fn early_exit() {
        let make_parser = |truncate: Option<usize>| async move {
            let src = include_bytes!("wire/../../testdata/demo.sendstream");
            // only use simplex stream if we're going to truncate it
            // prematurely, otherwise use a Cursor on top of the static byte
            // slice so that the framed codec actually sees the EOF
            let reader: Box<dyn AsyncRead + Unpin> = match truncate {
                Some(size) => {
                    let (receiver, mut sender) = tokio::io::simplex(size);
                    sender
                        .write_all(&src[..size])
                        .await
                        .expect("failed to write input data");
                    Box::new(receiver)
                }
                None => Box::new(Cursor::new(src)),
            };
            parse(reader)
        };

        // empirically determined that 46 commands fit in the first 4k
        // first, prove that we can read the expected number of commands, before
        // dropping the stream which while then stop consuming its upstream input
        let parser = make_parser(Some(4 * 1024)).await;
        let count = parser
            .take_until(sleep(Duration::from_millis(100)))
            .count()
            .await;
        assert_eq!(count, 46);

        // the entire stream should be consumable (but obviously only when given
        // the entire input)
        let parser = make_parser(None).await;
        let count = parser.count().await;
        assert_eq!(count, 106);
    }
}
