/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;

use nix::mount::MntFlags;
use nix::mount::MsFlags;
use proc_mounts::MountIter;
use tracing::info;

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
    #[error("Cannot parse /proc/mounts: {0:?}")]
    ParseError(std::io::Error),
    #[error("Unknown error occurred: {0:?}")]
    Unknown(#[from] nix::errno::Errno),
}

// We use this instead of proc_mounts::source_mounted_at to ignore possible iteration errors
pub fn source_mounted_at(source: &Path, target: &Path) -> Result<bool, MountError> {
    for mount in MountIter::new().map_err(MountError::ParseError)? {
        if let Ok(mount_info) = mount {
            if mount_info.source == source && mount_info.dest == target {
                return Ok(true);
            }
        }
    }

    Ok(false)
}

#[mockall::automock]
pub trait Mounter: Sized {
    fn mount<'a, 'b>(
        &'a self,
        source: &'b Path,
        target: &'b Path,
        fstype: Option<&'b str>,
        flags: MsFlags,
        data: Option<&'b str>,
    ) -> Result<MountHandle<'a, Self>, MountError>;

    fn umount(&self, mountpoint: &Path, force: bool) -> Result<(), nix::errno::Errno>;
}

// RealMounter is an implementation of the Mounter trait that calls nix::mount::mount for real.
pub struct RealMounter;

impl Mounter for RealMounter {
    fn mount<'a, 'b>(
        &'a self,
        source: &'b Path,
        target: &'b Path,
        fstype: Option<&'b str>,
        flags: MsFlags,
        data: Option<&'b str>,
    ) -> Result<MountHandle<'a, Self>, MountError> {
        info!(
            "Mounting {} to {} with fstype {:?}, flags {:?} and options {:?}",
            source.display(),
            target.display(),
            fstype,
            flags,
            data
        );
        match nix::mount::mount(Some(source), target, fstype, flags, data) {
            Ok(()) => Ok(MountHandle::new(target.to_path_buf(), self)),
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
        info!("Unmounting {} with flags {:?}", mountpoint.display(), flags);
        nix::mount::umount2(mountpoint, flags)
    }
}

/// This mounter is bounded to live at most as long as the
/// mounter that it contains and will give out auto-unmounting
/// mounts. The primary use for this is to have mounts that aren't
/// meant to survive longer than something local to the binary.
/// For example if a loopback device is created and things are mounted from it
/// this can be used to ensure that the mounts are taken down before the loopback
/// device is detatched
pub struct BoundMounter<'a, M: Mounter>(&'a M);

impl<'a, M> BoundMounter<'a, M>
where
    M: Mounter,
{
    pub fn new(binding_reference: &'a M) -> Self {
        Self(binding_reference)
    }
}

impl<'limit, M> Mounter for BoundMounter<'limit, M>
where
    M: Mounter,
{
    fn mount<'a, 'b>(
        &'a self,
        source: &'b Path,
        target: &'b Path,
        fstype: Option<&'b str>,
        flags: MsFlags,
        data: Option<&'b str>,
    ) -> Result<MountHandle<'a, Self>, MountError> {
        match self.0.mount(source, target, fstype, flags, data) {
            Ok(mut handle) => {
                handle.auto_umount();
                Ok(handle.replace_mounter_unchecked(self))
            }
            Err(e) => Err(e),
        }
    }

    fn umount(&self, mountpoint: &Path, force: bool) -> Result<(), nix::errno::Errno> {
        self.0.umount(mountpoint, force)
    }
}

pub struct MountHandle<'a, M>
where
    M: Mounter,
{
    target: PathBuf,
    mounter: &'a M,
    auto_umount: bool,
    unmounted: bool,
}

impl<'a, M> MountHandle<'a, M>
where
    M: Mounter,
{
    fn new(target: PathBuf, mounter: &'a M) -> Self {
        Self {
            target,
            mounter,
            auto_umount: false,
            unmounted: false,
        }
    }

    pub fn umount(mut self, force: bool) -> Result<(), nix::errno::Errno> {
        self.mounter.umount(&self.target, force)?;
        self.unmounted = true;
        Ok(())
    }

    fn replace_mounter_unchecked<'b, NM: Mounter>(
        mut self,
        new_mounter: &'b NM,
    ) -> MountHandle<'b, NM> {
        let auto_umount = self.auto_umount;
        self.auto_umount = false;
        MountHandle {
            target: self.target.clone(),
            mounter: new_mounter,
            auto_umount,
            unmounted: false,
        }
    }

    pub fn auto_umount(&mut self) {
        self.auto_umount = true;
    }

    pub fn mountpoint(&self) -> &Path {
        &self.target
    }

    pub fn mount_relative<'b, 'c>(
        &'a self,
        source: &'c Path,
        relative_target: &'c Path,
        fstype: Option<&'c str>,
        flags: MsFlags,
        data: Option<&'c str>,
    ) -> Result<MountHandle<'b, M>, MountError>
    where
        'a: 'b,
    {
        let target = self.target.join(relative_target);
        let mut handle = self.mounter.mount(source, &target, fstype, flags, data)?;

        if self.auto_umount {
            handle.auto_umount();
        }
        Ok(handle)
    }
}

impl<'a, M> Drop for MountHandle<'a, M>
where
    M: Mounter,
{
    fn drop(&mut self) {
        if self.auto_umount && !self.unmounted {
            let _ = self.mounter.umount(&self.target, true);
            self.unmounted = true;
        }
    }
}
