/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Error;
use clap::Parser;

use crate::Result;

#[derive(Parser, Debug)]
/// Extract a tarball to a directory.
///
/// This is a simple wrapper around `tar` because it's easier to invoke from
/// `tarball.bzl:tarball_analyze` and is a point where we need to eventually add
/// more safety.
pub(crate) struct ExtractTarball {
    #[clap(long)]
    tar: PathBuf,
    #[clap(long)]
    out: PathBuf,
    /// TODO(vmagro): ensure that all files in the tar are owned by this
    /// user:group. In practice today this is always fine to ignore, but is not
    /// strictly correct
    #[clap(long = "user")]
    _user: String,
    #[clap(long = "group")]
    _group: String,
}

impl ExtractTarball {
    #[tracing::instrument(name = "extract-tarball", skip(self))]
    pub fn run(self) -> Result<()> {
        std::fs::create_dir(&self.out).context("while creating output directory")?;
        let mut cmd = Command::new("tar");
        if self.tar.extension() == Some(OsStr::new("zst")) {
            cmd.arg("-I").arg("zstd");
        }
        Err(Error::from(
            cmd.arg("-xf")
                .arg(&self.tar)
                .arg("-C")
                .arg(&self.out)
                .exec(),
        )
        .context("while execing tar")
        .into())
    }
}
