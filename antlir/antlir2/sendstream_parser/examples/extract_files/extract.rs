/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! Reconstruct regular files out of btrfs send-streams in pure userspace --
//! without `btrfs receive`, a btrfs filesystem, or root.
//!
//! Unlike the kernel `btrfs receive` path, this only rebuilds file *contents*
//! (`write` / `encoded_write` / `truncate`); ownership, xattrs, timestamps and
//! any directory structure beyond what a materialized file needs are ignored.
//! It exists for callers that must pull a specific file (e.g. a profiled binary)
//! out of a send-stream on a host that has no btrfs filesystem. A `clone` op
//! against a requested file is rejected rather than silently producing corrupt
//! contents.

use std::collections::HashMap;
use std::io::SeekFrom;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use futures::StreamExt;
use sendstream_parser::Command;
use sendstream_parser::EncodedWrite;
use sendstream_parser::wire;
use tokio::fs;
use tokio::fs::File;
use tokio::io::AsyncRead;
use tokio::io::AsyncSeekExt;
use tokio::io::AsyncWriteExt;

/// btrfs `BTRFS_ENCODED_IO_COMPRESSION_*` codec ids carried by encoded writes.
const COMPRESSION_NONE: u32 = 0;
const COMPRESSION_ZSTD: u32 = 2;

/// Materializes send-stream files whose base name matches one of `names` into an
/// output directory, preserving each file's in-stream relative path.
///
/// Apply streams in order via [`Extractor::extract`]: the full base subvolume
/// first, then any incremental layers stacked on top of it. Call
/// [`Extractor::finish`] to flush.
pub struct Extractor {
    output_dir: PathBuf,
    names: Vec<String>,
    open: HashMap<PathBuf, File>,
}

impl Extractor {
    pub fn new(output_dir: impl Into<PathBuf>, names: Vec<String>) -> Self {
        Self {
            output_dir: output_dir.into(),
            names,
            open: HashMap::new(),
        }
    }

    /// Parse and apply a single send-stream.
    pub async fn extract<R: AsyncRead + Unpin>(&mut self, reader: R) -> Result<()> {
        let mut stream = wire::parse(reader);
        while let Some(cmd) = stream.next().await {
            let cmd = cmd.context("parsing send-stream")?;
            self.apply(cmd).await?;
        }
        Ok(())
    }

    /// Flush every materialized file and return how many were written.
    pub async fn finish(mut self) -> Result<usize> {
        let count = self.open.len();
        for file in self.open.values_mut() {
            file.flush().await?;
        }
        Ok(count)
    }

    async fn apply(&mut self, cmd: Command) -> Result<()> {
        match cmd {
            Command::Write(w) => {
                let Some(rel) = self.matched(w.path()) else {
                    return Ok(());
                };
                let rel = rel.to_path_buf();
                let offset = w.offset().as_u64();
                let file = self.file_for(&rel).await?;
                write_at(file, offset, w.data().as_slice()).await
            }
            Command::EncodedWrite(ew) => {
                let Some(rel) = self.matched(ew.path()) else {
                    return Ok(());
                };
                let rel = rel.to_path_buf();
                let offset = ew.offset().as_u64();
                let data = decode_extent(&ew)?;
                let file = self.file_for(&rel).await?;
                write_at(file, offset, &data).await
            }
            Command::Truncate(t) => {
                let Some(rel) = self.matched(t.path()) else {
                    return Ok(());
                };
                let rel = rel.to_path_buf();
                let size = t.size();
                let file = self.file_for(&rel).await?;
                Ok(file.set_len(size).await?)
            }
            Command::Clone(c) => {
                if self.matched(c.dst_path()).is_some() {
                    bail!(
                        "{} is reconstructed via a clone op referencing another extent, which is not supported",
                        c.dst_path().display()
                    );
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// Returns the in-stream path when its final component matches a target name.
    fn matched<'p>(&self, path: &'p Path) -> Option<&'p Path> {
        let base = path.file_name()?.to_str()?;
        self.names.iter().any(|n| n == base).then_some(path)
    }

    async fn file_for(&mut self, rel: &Path) -> Result<&mut File> {
        if !self.open.contains_key(rel) {
            let dst = self.output_dir.join(rel);
            if let Some(parent) = dst.parent() {
                fs::create_dir_all(parent).await?;
            }
            let file = File::create(&dst).await?;
            self.open.insert(rel.to_path_buf(), file);
        }
        self.open
            .get_mut(rel)
            .context("output file missing immediately after insertion")
    }
}

async fn write_at(file: &mut File, offset: u64, data: &[u8]) -> Result<()> {
    file.seek(SeekFrom::Start(offset)).await?;
    file.write_all(data).await?;
    Ok(())
}

/// Decodes the file bytes carried by an `encoded_write`: decompress `data` per
/// its codec, then take `[unencoded_offset, unencoded_offset + unencoded_file_len)`.
fn decode_extent(ew: &EncodedWrite) -> Result<Vec<u8>> {
    if ew.encryption().is_some() {
        bail!("encrypted send-stream extents are not supported");
    }
    decode_bytes(
        ew.compression().as_u32(),
        ew.data().as_slice(),
        ew.unencoded_offset().as_u64() as usize,
        ew.unencoded_file_len().as_u64() as usize,
    )
}

fn decode_bytes(codec: u32, data: &[u8], offset: usize, len: usize) -> Result<Vec<u8>> {
    let decoded = match codec {
        COMPRESSION_NONE => data.to_vec(),
        COMPRESSION_ZSTD => zstd::decode_all(data)?,
        other => bail!(
            "unsupported encoded-write compression codec {other} (only none and zstd are supported)"
        ),
    };

    let end = offset.saturating_add(len);
    if end > decoded.len() {
        bail!(
            "encoded-write slice {offset}..{end} is out of bounds for a {}-byte extent",
            decoded.len()
        );
    }
    Ok(decoded[offset..end].to_vec())
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn zstd_extent_round_trips_and_slices() {
        let original = b"the quick brown fox jumps over the lazy dog".to_vec();
        let compressed = zstd::encode_all(original.as_slice(), 0).expect("compress");

        let whole = decode_bytes(COMPRESSION_ZSTD, &compressed, 0, original.len())
            .expect("decode whole extent");
        assert_eq!(whole, original, "a zstd extent should round-trip exactly");

        // "the quick brown ..." -> bytes [4, 9) are "quick".
        let slice = decode_bytes(COMPRESSION_ZSTD, &compressed, 4, 5).expect("decode slice");
        assert_eq!(
            slice, b"quick",
            "unencoded_offset/len must select the file's bytes within the extent"
        );
    }

    #[test]
    fn uncompressed_extent_is_sliced_verbatim() {
        let got = decode_bytes(COMPRESSION_NONE, b"0123456789", 2, 3).expect("decode");
        assert_eq!(got, b"234");
    }

    #[test]
    fn unsupported_codec_is_rejected() {
        // zlib (1) and the lzo variants are not implemented.
        let err = decode_bytes(1, b"whatever", 0, 0)
            .expect_err("an unsupported codec must error rather than silently corrupt");
        assert!(
            err.to_string()
                .contains("unsupported encoded-write compression codec 1"),
            "error should name the unsupported codec, got: {err:#}"
        );
    }

    #[test]
    fn out_of_bounds_slice_is_rejected() {
        let err = decode_bytes(COMPRESSION_NONE, b"abc", 2, 5)
            .expect_err("a slice past the end of the extent must error");
        assert!(
            err.to_string().contains("out of bounds"),
            "a slice past the end of the extent must report it, got: {err:#}"
        );
    }
}
