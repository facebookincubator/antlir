/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use tempfile::NamedTempFile;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Btrfs {
    btrfs_packager_path: Vec<PathBuf>,
    spec: serde_json::Value,
}

impl Btrfs {
    pub fn build(&self, out: &Path) -> Result<()> {
        let btrfs_packager_path = self
            .btrfs_packager_path
            .first()
            .context("Expected exactly one arg to btrfs_packager_path")?;

        // The output path must exist before we can make an absolute path for it.
        let output_file = File::create(out).context("failed to create output file")?;
        output_file
            .sync_all()
            .context("Failed to sync output file to disk")?;
        drop(output_file);

        // Write just our sub-spec for btrfs to a file for the packager
        let btrfs_spec_file =
            NamedTempFile::new().context("failed to create tempfile for spec json")?;

        serde_json::to_writer(btrfs_spec_file.as_file(), &self.spec)
            .context("failed to write json to tempfile")?;

        btrfs_spec_file
            .as_file()
            .sync_all()
            .context("failed to sync json tempfile content")?;

        let btrfs_spec_file_abs = btrfs_spec_file
            .path()
            .canonicalize()
            .context("Failed to build abs path for spec tempfile")?;

        let mut btrfs_package_cmd = Command::new("sudo");
        btrfs_package_cmd
            .arg("unshare")
            .arg("--mount")
            .arg("--pid")
            .arg("--fork")
            .arg(btrfs_packager_path)
            .arg("--spec")
            .arg(btrfs_spec_file_abs)
            .arg("--out")
            .arg(out);

        let output = btrfs_package_cmd
            .output()
            .context("failed to spawn isolated btrfs-packager")?;

        println!(
            "btrfs-packager stdout:\n{}\nbtrfs-packager stderr\n{}",
            std::str::from_utf8(&output.stdout)
                .context("failed to render btrfs-packager stdout")?,
            std::str::from_utf8(&output.stderr)
                .context("failed to render btrfs-packager stderr")?,
        );

        match output.status.success() {
            true => Ok(()),
            false => Err(anyhow!(
                "failed to run command {:?}: {:?}",
                btrfs_package_cmd,
                output
            )),
        }
    }
}
