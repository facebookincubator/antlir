/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::{Path, PathBuf};

use nix::mount::{MntFlags, MsFlags};
use slog::{info, Logger};

#[derive(thiserror::Error, Debug)]
pub enum MountError {
    #[error("No such file or directory: Mount target {0:?} doesn't exist")]
    MissingSource(PathBuf),
    #[error("No such file or directory: Mount source {0:?} doesn't exist")]
    MissingTarget(PathBuf),
    #[error("No such file or directory: Unknown reason - both source/target exist")]
    MissingUnknown,
    #[error("Unknown error occured: {0:?}")]
    Unknown(#[from] nix::errno::Errno),
}

#[mockall::automock]
pub trait Mounter {
    fn mount<'a>(
        &'a self,
        source: &'a Path,
        target: &'a Path,
        fstype: Option<&'a str>,
        flags: MsFlags,
        data: Option<&'a str>,
    ) -> Result<(), MountError>;

    fn umount(&self, mountpoint: &Path, force: bool) -> Result<(), nix::errno::Errno>;
}

// RealMounter is an implementation of the Mounter trait that calls nix::mount::mount for real.
pub struct RealMounter {
    pub log: Logger,
}

impl Mounter for RealMounter {
    fn mount(
        &self,
        source: &Path,
        target: &Path,
        fstype: Option<&str>,
        flags: MsFlags,
        data: Option<&str>,
    ) -> Result<(), MountError> {
        info!(
            self.log,
            "Mounting {} to {} with fstype {:?}, flags {:?} and options {:?}",
            source.display(),
            target.display(),
            fstype,
            flags,
            data
        );
        match nix::mount::mount(Some(source), target, fstype, flags, data) {
            Ok(()) => Ok(()),
            Err(nix::errno::Errno::ENOENT) => Err(if !target.exists() {
                MountError::MissingTarget(target.to_path_buf())
            } else if !source.exists() {
                MountError::MissingSource(source.to_path_buf())
            } else {
                MountError::MissingUnknown
            }),
            Err(e) => Err(e.into()),
        }
    }

    fn umount(&self, mountpoint: &Path, force: bool) -> Result<(), nix::errno::Errno> {
        let mut flags = MntFlags::empty();
        if force {
            flags.insert(MntFlags::MNT_FORCE);
        }
        info!(
            self.log,
            "Unmounting {} with flags {:?}",
            mountpoint.display(),
            flags
        );
        nix::mount::umount2(mountpoint, flags)
    }
}
