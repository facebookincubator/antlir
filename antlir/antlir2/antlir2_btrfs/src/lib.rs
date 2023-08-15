/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(exit_status_error)]

use std::ffi::CStr;
use std::ffi::CString;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use bitflags::bitflags;
use btrfsutil_sys::btrfs_util_create_snapshot_fd;
use btrfsutil_sys::btrfs_util_create_subvolume;
use btrfsutil_sys::btrfs_util_delete_subvolume;
use btrfsutil_sys::btrfs_util_error;
use btrfsutil_sys::btrfs_util_is_subvolume_fd;
use btrfsutil_sys::btrfs_util_set_subvolume_read_only_fd;
use btrfsutil_sys::btrfs_util_strerror;
use btrfsutil_sys::btrfs_util_subvolume_id_fd;
use btrfsutil_sys::BTRFS_UTIL_CREATE_SNAPSHOT_READ_ONLY;
use btrfsutil_sys::BTRFS_UTIL_CREATE_SNAPSHOT_RECURSIVE;
use btrfsutil_sys::BTRFS_UTIL_DELETE_SUBVOLUME_RECURSIVE;
use nix::dir::Dir;
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;
use thiserror::Error;

pub static BTRFS_FS_TREE_OBJECTID: u64 = 5;

#[derive(Debug, Error)]
pub enum Error {
    #[error("btrfsutil error {0:?}")]
    Btrfs(#[from] BtrfsUtilError),
    #[error(transparent)]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Uncategorized(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct BtrfsUtilError {
    pub code: btrfs_util_error,
    pub msg: String,
}

impl From<btrfs_util_error> for BtrfsUtilError {
    fn from(err: btrfs_util_error) -> Self {
        let msg = unsafe {
            let msg = btrfs_util_strerror(err);
            CStr::from_ptr(msg)
        };
        Self {
            code: err,
            msg: format!("btrfs error {}: {:?}", err, msg),
        }
    }
}

impl std::fmt::Display for BtrfsUtilError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "btrfs_util_error({}, {})", self.code, self.msg)
    }
}

impl std::error::Error for BtrfsUtilError {}

/// Convenience macro to call btrfs_util functions that return a
/// btrfs_util_error. Code inside the macro call will be evaluated and a
/// `Result` will be returned.
macro_rules! check {
    ($code:block) => {{
        let ret = unsafe { $code };
        match ret {
            0 => Ok(()),
            _ => Err(BtrfsUtilError::from(ret)),
        }
    }};
}

#[derive(Debug)]
pub struct Subvolume {
    fd: Dir,
    id: u64,
    opened_path: PathBuf,
}

bitflags! {
    pub struct SnapshotFlags: i32 {
        const RECURSIVE = BTRFS_UTIL_CREATE_SNAPSHOT_RECURSIVE as i32;
        const READONLY = BTRFS_UTIL_CREATE_SNAPSHOT_READ_ONLY as i32;
    }
}

bitflags! {
    pub struct DeleteFlags: i32 {
        const RECURSIVE = BTRFS_UTIL_DELETE_SUBVOLUME_RECURSIVE as i32;
    }
}

impl Subvolume {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let fd = Dir::open(path.as_ref(), OFlag::empty(), Mode::empty())
            .map_err(std::io::Error::from)?;
        check!({ btrfs_util_is_subvolume_fd(fd.as_raw_fd()) })?;
        let mut id = unsafe { std::mem::zeroed() };
        check!({ btrfs_util_subvolume_id_fd(fd.as_raw_fd(), &mut id) })?;

        // if we ever want to delete it, we need to remember the original path
        // where it was opened :/
        let opened_path = path.as_ref().canonicalize()?;
        Ok(Self {
            fd,
            id,
            opened_path,
        })
    }

    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let cpath = CString::new(path.as_ref().as_os_str().as_bytes())
            .context("failed to convert path to CString")?;
        check!({
            btrfs_util_create_subvolume(
                cpath.as_ptr(),
                0,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        })
        .with_context(|| format!("failed to create subvol at {}", path.as_ref().display()))?;
        Self::open(path)
    }

    /// Path where this subvolume was opened. May not be the only path where
    /// this subvolume exists.
    pub fn path(&self) -> &Path {
        &self.opened_path
    }

    pub fn snapshot(&self, path: impl AsRef<Path>, flags: SnapshotFlags) -> Result<Self> {
        let snapshot_path = CString::new(path.as_ref().as_os_str().as_bytes())
            .context("failed to convert path to CString")?;
        check!({
            btrfs_util_create_snapshot_fd(
                self.fd.as_raw_fd(),
                snapshot_path.as_ptr(),
                flags.bits(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        })?;
        Self::open(path)
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn set_readonly(&mut self, ro: bool) -> Result<()> {
        check!({ btrfs_util_set_subvolume_read_only_fd(self.fd.as_raw_fd(), ro) })?;
        Ok(())
    }

    pub fn delete(self, flags: DeleteFlags) -> std::result::Result<(), (Self, Error)> {
        let opened_path = CString::new(self.opened_path.as_os_str().as_bytes())
            .expect("failed to convert path to CString");
        check!({ btrfs_util_delete_subvolume(opened_path.as_ptr(), flags.bits()) })
            .map_err(|e| (self, e.into()))?;
        Ok(())
    }
}

#[cfg(test)]
#[allow(non_upper_case_globals)]
mod tests {
    use btrfsutil_sys::btrfs_util_error_BTRFS_UTIL_ERROR_NOT_SUBVOLUME;

    use super::*;

    #[test]
    fn get_root() -> Result<()> {
        let subvol = Subvolume::open("/")?;
        assert!(subvol.id() != 0);
        Ok(())
    }

    /// Ensure that when we ask for a subvolume with a path to a non-non-subvol, we get
    /// an error rather than a footgun
    #[test]
    fn bad_get() -> Result<()> {
        std::fs::create_dir("/foo").expect("dir creation failed");
        std::fs::create_dir("/foo/bar").expect("subdir creation failed");
        assert!(
            matches!(
                Subvolume::open("/foo/bar"),
                Err(Error::Btrfs(BtrfsUtilError {
                    code: btrfs_util_error_BTRFS_UTIL_ERROR_NOT_SUBVOLUME,
                    ..
                }))
            ),
            "expected error on subvol lookup for regular directory"
        );
        Ok(())
    }

    #[test]
    fn snapshot() -> Result<()> {
        let subvol = Subvolume::open("/")?;
        let snap = subvol.snapshot("/snapshot", SnapshotFlags::empty())?;
        assert_eq!(snap.path(), Path::new("/snapshot"));
        assert!(snap.path().join("empty").exists());
        Ok(())
    }
}
