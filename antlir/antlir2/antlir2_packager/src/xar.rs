/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use tempfile::TempDir;

use crate::run_cmd;
use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
pub struct Xar {
    make_xar: Vec<String>,
    layer: PathBuf,
    executable: PathBuf,
}

impl PackageFormat for Xar {
    fn build(&self, out: &Path) -> Result<()> {
        let layer_canonical = self.layer.canonicalize().with_context(|| {
            format!("failed to canonicalize layer path {}", self.layer.display())
        })?;

        let tmpdir = TempDir::new_in("/tmp").context("while making tmp dir")?;

        let mut make_xar_cmd = self.make_xar.iter();
        let make_xar = make_xar_cmd.next().context("make_xar command empty")?;
        run_cmd(
            Command::new(make_xar)
                .args(make_xar_cmd)
                .arg("--output")
                .arg(out)
                .arg("--raw")
                .arg(&layer_canonical)
                .arg("--raw-executable")
                .arg(
                    self.executable
                        .strip_prefix("/")
                        .unwrap_or(&self.executable),
                )
                .env("TMPDIR", tmpdir.path())
                .stdout(Stdio::piped()),
        )
        .context("while running make_xar")?;

        Ok(())
    }
}
