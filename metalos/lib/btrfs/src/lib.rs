/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(exit_status_error)]

use std::collections::BTreeMap;
use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::OsStr;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;

use anyhow::Context;
use bitflags::bitflags;
use libc::c_void;
use thiserror::Error;
use uuid::Uuid;

pub mod sendstream;
pub use sendstream::Sendstream;
pub use sendstream::SendstreamExt;

use btrfsutil_sys::btrfs_util_create_snapshot;
use btrfsutil_sys::btrfs_util_create_subvolume;
use btrfsutil_sys::btrfs_util_create_subvolume_iterator;
use btrfsutil_sys::btrfs_util_delete_subvolume;
use btrfsutil_sys::btrfs_util_destroy_subvolume_iterator;
use btrfsutil_sys::btrfs_util_error;
use btrfsutil_sys::btrfs_util_error_BTRFS_UTIL_ERROR_STOP_ITERATION as BTRFS_UTIL_ERROR_STOP_ITERATION;
use btrfsutil_sys::btrfs_util_set_subvolume_read_only;
use btrfsutil_sys::btrfs_util_strerror;
use btrfsutil_sys::btrfs_util_subvolume_id;
use btrfsutil_sys::btrfs_util_subvolume_info;
use btrfsutil_sys::btrfs_util_subvolume_iterator;
use btrfsutil_sys::btrfs_util_subvolume_iterator_next_info;
use btrfsutil_sys::BTRFS_SUBVOL_RDONLY;
use btrfsutil_sys::BTRFS_UTIL_CREATE_SNAPSHOT_READ_ONLY;
use btrfsutil_sys::BTRFS_UTIL_CREATE_SNAPSHOT_RECURSIVE;
use btrfsutil_sys::BTRFS_UTIL_DELETE_SUBVOLUME_RECURSIVE;

pub static BTRFS_FS_TREE_OBJECTID: u64 = 5;

#[derive(Debug, Error)]
pub enum Error {
    #[error("btrfsutil error {0:?}")]
    Btrfs(#[from] BtrfsUtilError),
    #[error("sendstream error {0:?}")]
    Sendstream(#[from] sendstream::Error),
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

#[derive(Debug, Clone)]
pub struct Subvolume {
    id: u64,
    path: PathBuf,
    info: SubvolumeInfo,
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
    pub fn get(path: impl AsRef<Path>) -> Result<Self> {
        // The path stored in the Subvolume may be referenced later, so for
        // simplicity just canonicalize it to an absolute path immediately.
        let path = std::fs::canonicalize(&path)
            .with_context(|| format!("failed to canonicalize '{}'", path.as_ref().display()))?;
        let cpath = CString::new(path.as_os_str().as_bytes())
            .context("failed to convert path to CString")?;
        let mut id = 0;
        check!({ btrfs_util_subvolume_id(cpath.as_ptr(), &mut id) })
            .with_context(|| format!("failed to get subvol id for {}", path.display()))?;
        let mut info = unsafe { std::mem::zeroed() };
        check!({ btrfs_util_subvolume_info(cpath.as_ptr(), id, &mut info) })
            .with_context(|| format!("failed to get subvol info for {}", path.display()))?;
        Ok(Self {
            id,
            path,
            info: info.into(),
        })
    }

    pub fn root() -> Result<Self> {
        Self::get("/")
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
        Self::get(path)
    }

    pub fn snapshot(&self, path: impl AsRef<Path>, flags: SnapshotFlags) -> Result<Self> {
        let snapshot_path = CString::new(path.as_ref().as_os_str().as_bytes())
            .context("failed to convert path to CString")?;
        check!({
            btrfs_util_create_snapshot(
                self.path_cstr()?.as_ptr(),
                snapshot_path.as_ptr(),
                flags.bits(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
            )
        })
        .with_context(|| format!("failed to create snapshot at {}", path.as_ref().display()))?;
        Self::get(path)
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn path_cstr(&self) -> anyhow::Result<CString> {
        CString::new(self.path.as_os_str().as_bytes())
            .with_context(|| format!("failed to convert {:?} to CString", self.path))
    }

    pub fn info(&self) -> &SubvolumeInfo {
        &self.info
    }

    /// Load all subvolumes reachable from '/' and create a map keyed by their
    /// UUID. This is all subvolumes that are under subvolid 5.
    pub fn all_subvols_by_uuid() -> Result<BTreeMap<Uuid, Subvolume>> {
        Self::create_iterator(Path::new("/"), BTRFS_FS_TREE_OBJECTID)?
            .map(|subvol| subvol.map(|subvol| (subvol.info.uuid, subvol)))
            .collect()
    }

    fn create_iterator(path: &Path, subvol_id: u64) -> Result<SubvolIterator> {
        let cpath = CString::new(path.as_os_str().as_bytes())
            .context("failed to convert path to CString")?;
        let mut iter = unsafe { std::mem::zeroed() };
        check!({ btrfs_util_create_subvolume_iterator(cpath.as_ptr(), subvol_id, 0, &mut iter) })
            .with_context(|| format!("failed to make iterator for {}", path.display()))?;
        Ok(SubvolIterator(path.to_owned(), iter))
    }

    pub fn children(&self) -> Result<SubvolIterator> {
        Self::create_iterator(&self.path, self.id)
    }

    pub fn is_readonly(&self) -> bool {
        self.info.flags.contains(SubvolumeInfoFlags::READONLY)
    }

    pub fn set_readonly(&mut self, ro: bool) -> Result<()> {
        let path = self.path_cstr()?;
        check!({ btrfs_util_set_subvolume_read_only(path.as_ptr(), ro) })
            .with_context(|| format!("while setting subvolid={} ro={}", self.id, ro))?;
        self.info.flags.set(SubvolumeInfoFlags::READONLY, ro);
        Ok(())
    }

    pub fn delete(self, flags: DeleteFlags) -> Result<()> {
        let path = self.path_cstr()?;
        check!({ btrfs_util_delete_subvolume(path.as_ptr(), flags.bits()) }).with_context(
            || {
                format!(
                    "while deleting subvolid={} path={}",
                    self.id,
                    self.path().display()
                )
            },
        )?;
        Ok(())
    }
}

#[derive(Debug, Clone)]
pub struct SubvolumeInfo {
    pub id: u64,
    pub parent_id: Option<u64>,
    pub dir_id: Option<u64>,
    pub flags: SubvolumeInfoFlags,
    pub uuid: Uuid,
    pub parent_uuid: Option<Uuid>,
    pub received_uuid: Option<Uuid>,
    pub generation: u64,
    pub ctransid: u64,
    pub otransid: u64,
    pub stransid: Option<u64>,
    pub rtransid: Option<u64>,
}

bitflags! {
    pub struct SubvolumeInfoFlags: u64 {
        const READONLY = BTRFS_SUBVOL_RDONLY as u64;
    }
}

fn optional_u64(x: u64) -> Option<u64> {
    match x {
        0 => None,
        _ => Some(x),
    }
}

fn uuid(uuid: [u8; 16usize]) -> Option<Uuid> {
    match uuid.iter().all(|i| *i == 0) {
        true => None,
        false => Some(Uuid::from_bytes(uuid)),
    }
}

impl From<btrfsutil_sys::btrfs_util_subvolume_info> for SubvolumeInfo {
    fn from(i: btrfsutil_sys::btrfs_util_subvolume_info) -> Self {
        Self {
            id: i.id,
            parent_id: optional_u64(i.parent_id),
            dir_id: optional_u64(i.dir_id),
            // Some flags may be undefined, but we should preserve extra bits
            // even if we don't have proper names for them, hence the unsafe
            flags: unsafe { SubvolumeInfoFlags::from_bits_unchecked(i.flags) },
            uuid: uuid(i.uuid).expect("subvol uuid should be nonzero"),
            parent_uuid: uuid(i.parent_uuid),
            received_uuid: uuid(i.received_uuid),
            generation: i.generation,
            ctransid: i.ctransid,
            otransid: i.otransid,
            stransid: optional_u64(i.stransid),
            rtransid: optional_u64(i.rtransid),
        }
    }
}

pub struct SubvolIterator(PathBuf, *mut btrfs_util_subvolume_iterator);

impl Iterator for SubvolIterator {
    type Item = Result<Subvolume>;

    fn next(&mut self) -> Option<Self::Item> {
        let mut info = unsafe { std::mem::zeroed() };
        let mut path = unsafe { std::mem::zeroed() };
        match check!({ btrfs_util_subvolume_iterator_next_info(self.1, &mut path, &mut info) }) {
            Ok(()) => {
                let path_cstr = unsafe { CStr::from_ptr(path) };
                // Paths returned by the iterator are relative to the subvol
                // that the iteration was started from, for convenience join
                // them to the parent so that they are absolute
                let subvol_path = self.0.join(OsStr::from_bytes(path_cstr.to_bytes()));
                unsafe { libc::free(path as *mut c_void) };
                let info: SubvolumeInfo = info.into();
                Some(Ok(Subvolume {
                    id: info.id,
                    path: subvol_path,
                    info,
                }))
            }
            Err(e) => match e.code {
                BTRFS_UTIL_ERROR_STOP_ITERATION => None,
                _ => Some(Err(Error::Uncategorized(anyhow::Error::msg(e)))),
            },
        }
    }
}

impl Drop for SubvolIterator {
    fn drop(&mut self) {
        unsafe { btrfs_util_destroy_subvolume_iterator(self.1) };
    }
}

pub(crate) mod __private {
    pub trait Sealed {}
}

#[cfg(test)]
mod tests {
    use super::*;
    use metalos_macros::containertest;

    #[containertest]
    fn get_root() -> Result<()> {
        let subvol = Subvolume::get("/")?;
        assert!(subvol.info().id != 0);
        Ok(())
    }

    #[containertest]
    fn iter_root() -> Result<()> {
        let subvol = Subvolume::get("/")?;
        // systemd creates other subvols like /var/lib/portable, so just make
        // sure that at least /example is here, and that there is one more
        // subvol than when we started
        let count = subvol.children()?.count();
        Subvolume::create("/example")?;
        let children: Vec<Subvolume> = subvol.children()?.collect::<Result<_>>()?;
        assert!(children.len() == count + 1);
        assert!(
            children
                .into_iter()
                .any(|c| c.path() == Path::new("/example")),
            "/example was not in root subvol iter"
        );
        Ok(())
    }

    #[containertest]
    fn uuid_map() -> Result<()> {
        let subvol = Subvolume::get("/")?;
        let all_subvols = Subvolume::all_subvols_by_uuid()?;
        assert!(all_subvols.contains_key(&subvol.info().uuid));
        Ok(())
    }

    #[containertest]
    fn snapshot() -> Result<()> {
        let subvol = Subvolume::get("/")?;
        let snap = subvol.snapshot("/snapshot", SnapshotFlags::empty())?;
        assert_eq!(snap.path(), Path::new("/snapshot"));
        assert!(snap.path().join("/etc/machine-id").exists());
        Ok(())
    }
}
