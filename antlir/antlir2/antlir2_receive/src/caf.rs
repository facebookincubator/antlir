/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use antlir2_btrfs::Subvolume;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;
use tracing::trace;

#[cfg(facebook)]
#[derive(Debug, Deserialize)]
struct CafPackage {
    name: String,
    uuid: String,
}

pub(super) fn recv_caf(src: &Path, dst: &Path) -> Result<()> {
    let par_tmpdir = tempfile::tempdir_in("/tmp").context("while creating tmpdir")?;
    trace!("created par unpack dir '{}'", par_tmpdir.path().display());
    std::fs::set_permissions(par_tmpdir.path(), std::fs::Permissions::from_mode(0o700))
        .context("while chmodding par tmpdir")?;
    let subvol = Subvolume::create(dst).context("while creating subvol")?;
    let src =
        std::fs::read_to_string(src).with_context(|| format!("while reading {}", src.display()))?;
    let src: CafPackage =
        serde_json::from_str(&src).with_context(|| format!("while parsing '{src}'"))?;
    let mut cmd = Command::new("fbpkg.fetch");
    cmd.arg(format!("{}:{}", src.name, src.uuid))
        // unpack into a tempdir that we know our user owns instead
        // of /tmp where this par has probably already been used by
        // root
        .env("FB_PAR_UNPACK_BASEDIR", par_tmpdir.path())
        .env("FB_PAR_UNPACK_ALLOW_EXTRA_OWNER_UIDS_UNSAFE", "65534")
        .current_dir(subvol.path());
    trace!("running {cmd:?}");
    let status = cmd
        .spawn()
        .context("while spawning fbpkg.fetch")?
        .wait()
        .context("while waiting for fbpkg.fetch")?;
    ensure!(status.success(), "fbpkg.fetch failed");
    Ok(())
}
