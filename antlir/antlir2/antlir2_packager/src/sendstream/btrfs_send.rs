/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::process::Command;
use std::process::Stdio;

use antlir2_btrfs::SnapshotFlags;
use antlir2_btrfs::Subvolume;
use antlir2_working_volume::WorkingVolume;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use retry::delay::Fixed;
use retry::retry;
use tempfile::NamedTempFile;
use tempfile::PersistError;
use tracing::trace;
use tracing::warn;

use super::Sendstream;

pub(super) fn build(spec: &Sendstream, out: &Path, layer: &Path) -> Result<()> {
    let rootless = antlir2_rootless::init().context("while initializing rootless")?;
    let canonical_layer = layer.canonicalize()?;
    let subvol = Subvolume::open(&canonical_layer).context("while opening subvol")?;
    let tempdir = tempfile::tempdir_in(canonical_layer.parent().expect("cannot be /"))
        .context("while creating temp dir")?;
    let snapshot_path = tempdir.path().join(&spec.volume_name);
    let mut snapshot = rootless.as_root(|| {
        subvol
            .snapshot(&snapshot_path, SnapshotFlags::READONLY)
            .with_context(|| {
                format!(
                    "while snapshotting to new subvol {}",
                    snapshot_path.display()
                )
            })
    })??;
    let file = retry(Fixed::from_millis(10_000).take(10), || {
        trace!(
            "attempting to send v1 {} sendstream into upgrader",
            snapshot.path().display(),
        );
        let file = NamedTempFile::new()?;
        rootless
            .as_root(|| {
                let mut cmd = Command::new("btrfs");
                cmd.arg("send");
                if let Some(parent) = &spec.incremental_parent {
                    cmd.arg("-p").arg(
                        parent
                            .canonicalize()
                            .with_context(|| format!("while resolving {}", parent.display()))?,
                    );
                }

                let mut btrfs_send = cmd
                    .arg(snapshot.path())
                    .stdout(Stdio::piped())
                    .spawn()
                    .context("while spawning btrfs-send")?;
                let upgrader =
                    buck_resources::get("antlir/antlir2/antlir2_packager/sendstream-upgrade")?;
                let mut upgrade = Command::new(upgrader)
                    .arg("--compression-level")
                    .arg(spec.compression_level.to_string())
                    .stdin(btrfs_send.stdout.take().expect("this is a pipe"))
                    .stdout(file.as_file().try_clone()?)
                    .spawn()
                    .context("while spawning sendstream-upgrade")?;

                if !btrfs_send
                    .wait()
                    .context("while waiting for btrfs-send")?
                    .success()
                {
                    return Err(anyhow!("btrfs-send failed"));
                }
                if !upgrade
                    .wait()
                    .context("while waiting for sendstream-upgrade")?
                    .success()
                {
                    return Err(anyhow!("sendstream-upgrade failed"));
                }
                Ok(file)
            })
            .context("rootless failed")?
    })
    .map_err(Error::msg)
    .context("btrfs-send failed too many times")?;

    if let Err(PersistError { file, error }) = file.persist(out) {
        warn!("failed to persist tempfile, falling back on full copy: {error:?}");
        std::fs::copy(file.path(), out).context("while copying sendstream to output")?;
    }

    let working_directory = canonical_layer.parent().expect("cannot be /").to_owned();
    let working_volume =
        WorkingVolume::ensure(working_directory).context("while initializing WorkingVolume")?;

    let final_path = working_volume.allocate_new_path()?;

    let subvol = rootless.as_root(|| {
        snapshot
            .set_readonly(false)
            .context("while making subvol r/w")?;
        std::fs::rename(snapshot.path(), &final_path).context("while moving subvol")?;
        let mut subvol = Subvolume::open(&final_path).context("while opening subvol")?;
        subvol
            .set_readonly(true)
            .context("while setting subvol readonly")?;
        Ok::<_, anyhow::Error>(subvol)
    })??;

    let out_subvol_symlink = spec
        .subvol_symlink
        .clone()
        .context("btrfs_send sendstreams must have subvol_symlink output")?;
    if out_subvol_symlink.exists() {
        trace!("removing existing output {}", out_subvol_symlink.display());
        rootless.as_root(|| match Subvolume::open(&out_subvol_symlink) {
            Ok(old_subvol) => {
                if let Err(e) = old_subvol.delete() {
                    warn!(
                        "couldn't delete old subvol '{}': {e:?}",
                        out_subvol_symlink.display()
                    );
                }
            }
            Err(e) => {
                warn!(
                    "couldn't open old subvol '{}': {e:?}",
                    out_subvol_symlink.display()
                );
            }
        })?;
    }

    let _ = std::fs::remove_file(&out_subvol_symlink);
    std::os::unix::fs::symlink(subvol.path(), &out_subvol_symlink)
        .context("while symlinking packaged subvol")?;

    Ok(())
}
