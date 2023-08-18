/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use retry::delay::Fixed;
use retry::retry;
use serde::Deserialize;
use tempfile::NamedTempFile;
use tempfile::PersistError;
use tracing::trace;
use tracing::warn;

use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
pub struct Sendstream {
    layer: PathBuf,
}

impl PackageFormat for Sendstream {
    fn build(&self, out: &Path) -> Result<()> {
        let v1file = retry(Fixed::from_millis(10_000).take(10), || {
            let v1file = NamedTempFile::new()?;
            trace!("sending v1 sendstream to {}", v1file.path().display());
            if Command::new("sudo")
                .arg("btrfs")
                .arg("send")
                .arg(&self.layer)
                .stdout(
                    v1file
                        .as_file()
                        .try_clone()
                        .context("while cloning v1file")?,
                )
                .spawn()
                .context("while spawning btrfs-send")?
                .wait()
                .context("while waiting for btrfs-send")?
                .success()
            {
                Ok(v1file)
            } else {
                Err(anyhow!("btrfs-send failed"))
            }
        })
        .map_err(Error::msg)
        .context("btrfs-send failed too many times")?;
        if let Err(PersistError { file, error }) = v1file.persist(&out) {
            warn!("failed to persist tempfile, falling back on full copy: {error:?}");
            std::fs::copy(file.path(), &out).context("while copying sendstream to output")?;
        }
        Ok(())
    }
}
