/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_btrfs::DeleteFlags;
use antlir2_btrfs::SnapshotFlags;
use antlir2_btrfs::Subvolume;
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
    volume_name: String,
    #[serde(default)]
    incremental_parent: Option<PathBuf>,
}

impl PackageFormat for Sendstream {
    fn build(&self, out: &Path) -> Result<()> {
        let rootless = antlir2_rootless::init().context("while initializing rootless")?;
        let subvol = Subvolume::open(&self.layer).context("while opening subvol")?;
        let tempdir =
            tempfile::tempdir_in(self.layer.canonicalize()?.parent().expect("cannot be /"))
                .context("while creating temp dir")?;
        let snapshot_path = tempdir.path().join(&self.volume_name);
        let snapshot = rootless.as_root(|| {
            let mut snapshot = subvol
                .snapshot(&snapshot_path, SnapshotFlags::RECURSIVE)
                .with_context(|| {
                    format!(
                        "while snapshotting to new subvol {}",
                        snapshot_path.display()
                    )
                })?;

            snapshot
                .set_readonly(true)
                .context("while setting snapshot readonly")?;
            Ok::<_, anyhow::Error>(snapshot)
        })??;
        let v1file = retry(Fixed::from_millis(10_000).take(10), || {
            let v1file = NamedTempFile::new()?;
            trace!("sending v1 sendstream to {}", v1file.path().display());
            rootless
                .as_root(|| {
                    let mut cmd = Command::new("btrfs");
                    cmd.arg("send");
                    if let Some(parent) = &self.incremental_parent {
                        cmd.arg("-p").arg(parent);
                    }

                    if cmd
                        .arg(snapshot.path())
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
                .context("rootless failed")?
        })
        .map_err(Error::msg)
        .context("btrfs-send failed too many times")?;
        rootless.as_root(|| {
            snapshot
                .delete(DeleteFlags::RECURSIVE)
                .map_err(|(_subvol, err)| err)
                .context("while deleting snapshot")
        })??;

        if let Err(PersistError { file, error }) = v1file.persist(out) {
            warn!("failed to persist tempfile, falling back on full copy: {error:?}");
            std::fs::copy(file.path(), out).context("while copying sendstream to output")?;
        }
        Ok(())
    }
}
