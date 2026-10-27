/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::bail;
use clap::Parser;
use snapshot_common::Checksums;

/// Verify a downloaded file against expected checksums.
///
/// Buck2's `download_file` only supports sha1/sha256 verification, but some
/// repositories (e.g. Ubuntu 26.10+) publish only SHA512 checksums. For those,
/// the build downloads with curl and pins the bytes with this subcommand,
/// which fails the action on mismatch.
///
/// When `--out` is given, the verified bytes are copied there on success, so
/// the verified copy can serve as the action's declared output.
#[derive(Debug, Parser)]
pub(crate) struct Verify {
    /// File to verify
    file: PathBuf,
    #[clap(long)]
    sha1: Option<String>,
    #[clap(long)]
    sha256: Option<String>,
    #[clap(long)]
    sha512: Option<String>,
    /// Copy the verified bytes here on success
    #[clap(long)]
    out: Option<PathBuf>,
}

impl Verify {
    pub(crate) fn run(&self) -> Result<()> {
        if self.sha1.is_none() && self.sha256.is_none() && self.sha512.is_none() {
            bail!("at least one of --sha1, --sha256 or --sha512 is required");
        }
        let expected = Checksums::builder()
            .maybe_sha1(self.sha1.clone())
            .maybe_sha256(self.sha256.clone())
            .maybe_sha512(self.sha512.clone())
            .build()?;
        let computed = Checksums::from_reader(
            std::fs::File::open(&self.file)
                .with_context(|| format!("failed to open {}", self.file.display()))?,
        )?;
        expected
            .verify_against(&computed)
            .with_context(|| format!("checksum mismatch for {}", self.file.display()))?;
        if let Some(out) = &self.out {
            std::fs::copy(&self.file, out).with_context(|| {
                format!(
                    "failed to copy {} to {}",
                    self.file.display(),
                    out.display()
                )
            })?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::io::Write as _;

    use super::*;

    fn write_temp_file(name: &str, contents: &[u8]) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "metadata-verify-test-{}-{}",
            std::process::id(),
            name
        ));
        let mut file = std::fs::File::create(&path).expect("failed to create temp file");
        file.write_all(contents).expect("failed to write temp file");
        path
    }

    #[test]
    fn test_verify_sha512_ok() {
        let path = write_temp_file("ok", b"Hello world\n");
        Verify {
            file: path.clone(),
            sha1: None,
            sha256: None,
            sha512: Some(
                "81381f1dacd4824a6c503fd07057763099c12b8309d0abcec4000c9060cbbfa67988b2ada669ab4837fcd3d4ea6e2b8db2b9da9197d5112fb369fd006da545de"
                    .to_owned(),
            ),
            out: None,
        }
        .run()
        .expect("matching sha512 should verify");
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_verify_sha512_mismatch() {
        let path = write_temp_file("mismatch", b"Hello world\n");
        let err = Verify {
            file: path.clone(),
            sha1: None,
            sha256: None,
            sha512: Some("bb".repeat(64)),
            out: None,
        }
        .run()
        .expect_err("mismatched sha512 should fail");
        assert!(
            format!("{err:#}").contains("mismatch"),
            "unexpected error: {err:#}"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_verify_requires_a_checksum() {
        let path = write_temp_file("empty", b"Hello world\n");
        let err = Verify {
            file: path.clone(),
            sha1: None,
            sha256: None,
            sha512: None,
            out: None,
        }
        .run()
        .expect_err("missing checksums should fail");
        assert!(
            format!("{err:#}").contains("at least one"),
            "unexpected error: {err:#}"
        );
        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn test_verify_copies_to_out() {
        let path = write_temp_file("copy-src", b"Hello world\n");
        let out = std::env::temp_dir().join(format!(
            "metadata-verify-test-{}-copy-dst",
            std::process::id()
        ));
        Verify {
            file: path.clone(),
            sha1: None,
            sha256: None,
            sha512: Some(
                "81381f1dacd4824a6c503fd07057763099c12b8309d0abcec4000c9060cbbfa67988b2ada669ab4837fcd3d4ea6e2b8db2b9da9197d5112fb369fd006da545de"
                    .to_owned(),
            ),
            out: Some(out.clone()),
        }
        .run()
        .expect("matching sha512 should verify and copy");
        assert_eq!(
            std::fs::read(&out).expect("failed to read copied file"),
            b"Hello world\n",
        );
        std::fs::remove_file(&path).ok();
        std::fs::remove_file(&out).ok();
    }
}
