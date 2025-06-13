/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(duration_constructors)]

use std::fmt::Debug;
use std::fs::OpenOptions;
use std::io::ErrorKind;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use nix::fcntl::Flock;
use nix::fcntl::FlockArg;
use nix::libc;
use tracing::trace;
use uuid::Uuid;

mod gc;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error("Failed to add redirect.\n{msg}: {cmd}\nError:\n{error}\nDebug info:\n{debug_info}")]
    AddRedirect {
        cmd: String,
        debug_info: String,
        error: String,
        msg: String,
    },
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
    _priv: (),
}

fn get_debug_info() -> String {
    let mut cmd = Command::new("eden");
    let res = cmd
        .arg("rage")
        .arg("--dry-run")
        .output()
        .map_err(|_| Some("None".to_string()))
        .expect("a string");
    format!(
        "\
        Eden doctor command: {cmd}\n\
        Eden doctor stdout:\n\
        {stdout}\n\
        Eden doctor stderr:\n\
        {stderr}",
        cmd = format_args!("{:?}", cmd),
        stdout = String::from_utf8_lossy(&res.stdout).into_owned(),
        stderr = String::from_utf8_lossy(&res.stderr).into_owned(),
    )
}

const DIRNAME: &str = "antlir2-out";

impl WorkingVolume {
    /// Ensure the [WorkingVolume] exists and is set up correctly.
    pub fn ensure() -> Result<Self> {
        // If we're on Eden, create a new redirection
        // https://www.internalfb.com/intern/wiki/EdenFS/detecting-an-eden-mount/#on-linux-and-macos
        match OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY)
            .open(".eden")
        {
            Ok(dir) => {
                // There seems to be some racy behavior with eden adding
                // redirects, take an exclusive lock before adding
                let _locked_dir = Flock::lock(dir, FlockArg::LockExclusive)
                    .map_err(|(_fd, err)| std::io::Error::from(err))?;
                if !std::fs::exists(DIRNAME).unwrap_or_default() {
                    let mut cmd = Command::new("eden");
                    let res = cmd
                        .env("EDENFSCTL_ONLY_RUST", "1")
                        .arg("redirect")
                        .arg("add")
                        .arg(DIRNAME)
                        .arg("bind")
                        .output()
                        .map_err(|e| Error::AddRedirect {
                            cmd: format!("{:?}", cmd),
                            debug_info: get_debug_info(),
                            error: e.to_string(),
                            msg: "Failed to run command".to_string(),
                        })?;
                    if !res.status.success() {
                        // Eden may still have created the directory before
                        // crashing. Attempt to clean it up so that future
                        // actions don't use it by mistake.
                        let _ = std::fs::remove_dir(DIRNAME);
                        return Err(Error::AddRedirect {
                            cmd: format!("{:?}", cmd),
                            debug_info: get_debug_info(),
                            error: String::from_utf8_lossy(&res.stderr).into_owned(),
                            msg: "Command failed".to_string(),
                        });
                    }
                }
            }
            Err(e) => match e.kind() {
                ErrorKind::NotFound => {
                    trace!("no .eden: {e:?}");
                    if let Err(e) = std::fs::create_dir(DIRNAME) {
                        if e.kind() != ErrorKind::AlreadyExists {
                            return Err(Error::CreateWorkingVolume(e));
                        }
                    }
                }
                _ => return Err(Error::CheckEden(e)),
            },
        };
        let s = Self { _priv: () };
        std::fs::create_dir_all(s.subvols_path()).map_err(Error::CreateWorkingVolume)?;
        Ok(s)
    }

    pub fn path(&self) -> &Path {
        Path::new(DIRNAME)
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
