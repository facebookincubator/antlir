/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::process::Command;

use antlir2_btrfs::Subvolume;
use antlir2_working_volume::WorkingVolume;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use tracing::trace;

use crate::Receive;

pub(crate) fn recv_sendstream(args: &Receive, dst: &Path) -> Result<()> {
    let wv = WorkingVolume::ensure()?;
    // make sure that working_dir is btrfs before we try to invoke
    // 'btrfs' so that we can fail with a nicely categorized error
    antlir2_btrfs::ensure_path_is_on_btrfs(wv.path())?;

    let recv_tmp = tempfile::tempdir_in(wv.path())?;
    let mut cmd = Command::new(&args.btrfs);
    cmd.arg("--quiet")
        .arg("receive")
        .arg(recv_tmp.path())
        .arg("-f")
        .arg(&args.source);
    if args.rootless {
        cmd.arg("--force-decompress");
    }
    trace!("receiving sendstream: {cmd:?}");
    let res = cmd.spawn()?.wait()?;
    ensure!(res.success(), "btrfs-receive failed");
    let entries: Vec<_> = std::fs::read_dir(&recv_tmp)
        .context("while reading tmp dir")?
        .map(|r| {
            r.map(|entry| entry.path())
                .context("while iterating tmp dir")
        })
        .collect::<anyhow::Result<_>>()?;
    ensure!(
        entries.len() == 1,
        "did not get exactly one subvolume received: {entries:?}"
    );

    trace!("opening received subvol: {}", entries[0].display());
    let mut subvol = Subvolume::open(&entries[0]).context("while opening subvol")?;
    subvol
        .set_readonly(false)
        .context("while making subvol rw")?;

    trace!(
        "moving received subvol to right location {} -> {}",
        subvol.path().display(),
        dst.display()
    );
    std::fs::rename(subvol.path(), dst).context("while renaming subvol")?;
    Ok(())
}
