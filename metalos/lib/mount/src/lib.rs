/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use antlir_image::layer::AntlirLayer;
use antlir_image::partition::Partition;
use antlir_image::path::VerifiedPath;
use antlir_image::subvolume::AntlirSubvolume;
use antlir_image::subvolume::AntlirSubvolumes;
use nix::mount::MntFlags;
use nix::mount::MsFlags;
use slog::info;
use slog::Logger;

#[derive(thiserror::Error, Debug)]
pub enum MountError {
    #[error("No such file or directory: Mount source {0:?} doesn't exist")]
    MissingSource(PathBuf),
    #[error("No such file or directory: Mount target {0:?} doesn't exist")]
    MissingTarget(PathBuf),
    #[error("No such file or directory: Unknown reason - both source/target exist")]
    MissingUnknown,
    #[error("Path {0:?} provided as subvolume path is not valid unicode")]
    InvalidSubvolume(PathBuf),
    #[error("Unknown error occurred: {0:?}")]
    Unknown(#[from] nix::errno::Errno),
}

pub trait SafeMounter {
    fn mount_btrfs<'a, S, L, F>(
        &'a self,
        source: &Partition<S>,
        target: VerifiedPath,
        flags: MsFlags,
        make_subvolume: F,
        //TODO support data
    ) -> Result<L, MountError>
    where
        S: AntlirSubvolumes,
        L: AntlirLayer,
        F: Fn(&Partition<S>) -> AntlirSubvolume<L>;

    fn umount(&self, mountpoint: &Path, force: bool) -> Result<(), nix::errno::Errno>;
}

pub struct RealSafeMounter {
    pub log: Logger,
}
impl SafeMounter for RealSafeMounter {
    fn mount_btrfs<'a, S, L, F>(
        &'a self,
        source: &Partition<S>,
        target: VerifiedPath,
        flags: MsFlags,
        make_subvolume: F,
    ) -> Result<L, MountError>
    where
        S: AntlirSubvolumes,
        L: AntlirLayer,
        F: Fn(&Partition<S>) -> AntlirSubvolume<L>,
    {
        let subvolume = make_subvolume(source);

        let data = format!(
            "subvol={}",
            match subvolume.relative_path.to_str() {
                Some(p) => p,
                None => {
                    return Err(MountError::InvalidSubvolume(
                        subvolume.relative_path.to_path_buf(),
                    ));
                }
            }
        );

        RealMounter {
            log: self.log.clone(),
        }
        .mount(
            source.path.path(),
            target.path(),
            Some("btrfs"),
            flags,
            Some(&data),
        )?;
        Ok(subvolume.mount_unchecked(target))
    }

    fn umount(&self, mountpoint: &Path, force: bool) -> Result<(), nix::errno::Errno> {
        RealMounter {
            log: self.log.clone(),
        }
        .umount(mountpoint, force)
    }
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
