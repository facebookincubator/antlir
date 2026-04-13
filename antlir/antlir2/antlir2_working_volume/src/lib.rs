/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(duration_constructors)]

use std::fmt::Debug;
use std::io::ErrorKind;
use std::os::unix::fs::MetadataExt as _;
use std::os::unix::fs::chown;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_rootless::Rootless;
use clap::ValueEnum;
use nix::unistd::getegid;
use nix::unistd::geteuid;
use tracing::trace;
use uuid::Uuid;

mod gc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("failed to get scratch path for working volume")]
    MkScratch(std::io::Error),
    #[error("failed to create working volume")]
    CreateWorkingVolume(std::io::Error),
    #[error("failed to check eden presence")]
    CheckEden(std::io::Error),
    #[error("garbage collection io error: {0}")]
    GarbageCollect(std::io::Error),
    #[error(transparent)]
    Btrfs(#[from] antlir2_btrfs::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug, Clone)]
pub struct WorkingVolume {
    path: PathBuf,
}

const DIRNAME: &str = "antlir2-out";

impl WorkingVolume {
    /// Ensure the [WorkingVolume] exists and is set up correctly.
    pub fn ensure() -> Result<Self> {
        // If we're on Eden, use mkscratch to get a scratch path on local disk.
        // https://www.internalfb.com/intern/wiki/EdenFS/detecting-an-eden-mount/#on-linux-and-macos
        let path = match std::fs::read_link(".eden/root") {
            Ok(repo_root) => {
                let output = Command::new("mkscratch")
                    .arg("path")
                    .arg(repo_root)
                    .arg("--subdir")
                    .arg(DIRNAME)
                    .output()
                    .map_err(Error::MkScratch)?;
                if !output.status.success() {
                    return Err(Error::MkScratch(std::io::Error::other(
                        String::from_utf8_lossy(&output.stderr).into_owned(),
                    )));
                }
                PathBuf::from(String::from_utf8_lossy(&output.stdout).trim().to_string())
            }
            Err(e) if e.kind() == ErrorKind::NotFound => {
                trace!("no .eden: {e:?}");
                if let Err(e) = std::fs::create_dir(DIRNAME) {
                    if e.kind() != ErrorKind::AlreadyExists {
                        return Err(Error::CreateWorkingVolume(e));
                    }
                }
                PathBuf::from(DIRNAME)
            }
            Err(e) => return Err(Error::CheckEden(e)),
        };
        let s = Self { path };
        let subvols_dir = s.subvols_path();
        if let Err(e) = std::fs::create_dir(&subvols_dir) {
            if e.kind() != ErrorKind::AlreadyExists {
                return Err(Error::CreateWorkingVolume(e));
            }
        }
        // TODO(vmagro): remove this in a couple weeks once all the
        // currently-incorrect permissions have been fixed
        let rootless = Rootless::get_if_initialized();
        let meta = std::fs::metadata(&subvols_dir)?;
        let expected_uid = rootless
            .and_then(|r| r.unprivileged_uid())
            .unwrap_or_else(geteuid)
            .as_raw();
        let expected_gid = rootless
            .and_then(|r| r.unprivileged_gid())
            .unwrap_or_else(getegid)
            .as_raw();
        let mut fix_with_sudo = false;
        if meta.uid() != expected_uid || meta.gid() != expected_gid {
            if let Some(rootless) = rootless {
                if let Err(_e) = rootless
                    .as_root(|| chown(&subvols_dir, Some(expected_uid), Some(expected_gid)))
                    .map_err(std::io::Error::other)
                {
                    fix_with_sudo = true;
                }
            } else {
                fix_with_sudo = true;
            }
        }
        if fix_with_sudo {
            let out = Command::new("sudo")
                .arg("chown")
                .arg(format!("{expected_uid}:{expected_gid}"))
                .arg(&subvols_dir)
                .output()?;
            if !out.status.success() {
                return Err(std::io::Error::other(format!(
                    "Failed to chown {subvols_dir:?} to {expected_uid}:{expected_gid}: {}",
                    String::from_utf8_lossy(&out.stderr)
                ))
                .into());
            }
        }
        Ok(s)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn subvols_path(&self) -> PathBuf {
        self.path().join("subvols")
    }

    /// Provide a new (non-existent) path for an image build to put its result
    /// into.
    pub fn allocate_new_subvol_path(&self) -> Result<PathBuf> {
        Ok(self
            .subvols_path()
            .join(Uuid::new_v4().simple().to_string()))
    }
}

#[derive(Debug, ValueEnum, Clone, Copy, PartialEq, Eq)]
pub enum WorkingFormat {
    Btrfs,
    CadStack,
}
