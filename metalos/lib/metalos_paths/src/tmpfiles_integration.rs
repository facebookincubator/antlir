/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(exit_status_error)]
use std::ffi::CString;
use std::fs::File;
use std::io::Seek;
use std::io::Write;
use std::os::unix::io::FromRawFd;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;

// Ensure that the subvolume hierarchy in the control volume matches what this
// code expects.
pub fn setup_tmpfiles() -> Result<()> {
    let mut stdin = unsafe {
        File::from_raw_fd(nix::sys::memfd::memfd_create(
            &CString::new("input").expect("creating cstr can never fail with this static input"),
            nix::sys::memfd::MemFdCreateFlag::empty(),
        )?)
    };
    stdin
        .write_all(include_bytes!("../metalos_paths.tmpfiles.conf"))
        .context("while writing rules to stdin")?;
    stdin.rewind().context("while preparing stdin")?;

    let out = Command::new("systemd-tmpfiles")
        .env("SYSTEMD_LOG_LEVEL", "debug")
        // Without this force env var, systemd-tmpfiles will only create subvols
        // if / is btrfs, which it is not in the initrd, even though we asked
        // for subvols and we gave a path that is btrfs...
        .env("SYSTEMD_TMPFILES_FORCE_SUBVOL", "1")
        .arg("--create")
        .arg("-")
        .stdin(stdin)
        .output()
        .context("failed to start systemd-tmpfiles --create -")?;
    out.status.exit_ok().with_context(|| {
        format!(
            "'systemd-tmpfiles --create -' failed: {}\nrules:\n{}",
            std::str::from_utf8(&out.stderr).unwrap_or("<invalid utf8>"),
            include_str!("../metalos_paths.tmpfiles.conf"),
        )
    })?;
    Ok(())
}
