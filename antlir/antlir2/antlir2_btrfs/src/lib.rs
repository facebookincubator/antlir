/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(exit_status_error)]
#![cfg_attr(test, feature(io_error_more))]

use std::ffi::OsStr;
use std::fmt::Debug;
use std::fs::OpenOptions;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::path::Path;
use std::path::PathBuf;

use bitflags::bitflags;
use nix::dir::Dir;
use nix::errno::Errno;
use nix::fcntl::OFlag;
use nix::sys::stat::fstat;
use nix::sys::stat::Mode;
use nix::sys::statfs::fstatfs;
use nix::sys::statfs::BTRFS_SUPER_MAGIC;
use thiserror::Error;
use tracing::trace;
use tracing::trace_span;
use uuid::Uuid;

const INO_SUBVOL: u64 = 256;

mod ioctl;

#[derive(Debug, Error)]
pub enum Error {
    #[error("not a btrfs filesystem")]
    NotBtrfs,
    #[error("directory is not a btrfs subvolume")]
    NotSubvol,
    #[error("cannot delete root subvolume")]
    CannotDeleteRoot,
    #[error("cannot create subvolume at /")]
    CannotCreateRoot,
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Debug)]
pub struct Subvolume {
    fd: Dir,
    parent: Option<Dir>,
    id: u64,
    opened_path: PathBuf,
}

bitflags! {
    #[derive(Default, PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
    pub struct SnapshotFlags: u64 {
        const READONLY = 1 << 1;
    }
}

bitflags! {
    #[derive(PartialEq, Eq, PartialOrd, Ord, Hash, Debug, Clone, Copy)]
    struct SubvolFlags: u64 {
        const READ_ONLY = 1 << 1;
    }
}

fn name_bytes<const L: usize>(name: &OsStr) -> [u8; L] {
    let mut buf = [0; L];
    let name_bytes = name.as_bytes();
    buf[..name_bytes.len()].copy_from_slice(name_bytes);
    buf
}

fn ensure_is_btrfs(fd: &impl AsRawFd) -> Result<()> {
    let statfs = fstatfs(fd).map_err(std::io::Error::from)?;
    if statfs.filesystem_type() != BTRFS_SUPER_MAGIC {
        return Err(Error::NotBtrfs);
    }
    Ok(())
}

impl Subvolume {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let parent = match path.as_ref().parent() {
            Some(parent) => Some(
                Dir::open(
                    if parent == Path::new("") {
                        Path::new(".")
                    } else {
                        parent
                    },
                    OFlag::O_DIRECTORY | OFlag::O_RDONLY,
                    Mode::empty(),
                )
                .map_err(std::io::Error::from)?,
            ),
            None => None,
        };
        let fd = Dir::open(path.as_ref(), OFlag::O_DIRECTORY, Mode::empty())
            .map_err(std::io::Error::from)?;

        ensure_is_btrfs(&fd)?;

        let stat = fstat(fd.as_raw_fd()).map_err(std::io::Error::from)?;
        if stat.st_ino != INO_SUBVOL {
            return Err(Error::NotSubvol);
        }

        let mut args = ioctl::ino_lookup_args {
            objectid: ioctl::FIRST_FREE_OBJECTID,
            ..Default::default()
        };
        unsafe {
            ioctl::ino_lookup(fd.as_raw_fd(), &mut args).map_err(std::io::Error::from)?;
        }

        let id = args.treeid;

        // if we ever want to delete it, we need to remember the original path
        // where it was opened :/
        let opened_path = path.as_ref().canonicalize()?;
        Ok(Self {
            fd,
            parent,
            id,
            opened_path,
        })
    }

    pub fn create(path: impl AsRef<Path>) -> Result<Self> {
        let parent_fd = Dir::open(
            path.as_ref().parent().ok_or(Error::CannotCreateRoot)?,
            OFlag::O_DIRECTORY,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;

        ensure_is_btrfs(&parent_fd)?;

        let args = ioctl::vol_args_v2 {
            id: ioctl::vol_args_v2_spec {
                name: name_bytes(path.as_ref().file_name().ok_or(Error::CannotCreateRoot)?),
            },
            ..Default::default()
        };
        unsafe {
            ioctl::subvol_create_v2(parent_fd.as_raw_fd(), &args).map_err(std::io::Error::from)?;
        }

        Self::open(path)
    }

    /// Path where this subvolume was opened. May not be the only path where
    /// this subvolume exists.
    pub fn path(&self) -> &Path {
        &self.opened_path
    }

    pub fn snapshot(&self, path: impl AsRef<Path>, flags: SnapshotFlags) -> Result<Self> {
        let new_parent_fd = Dir::open(
            path.as_ref().parent().ok_or(Error::CannotCreateRoot)?,
            OFlag::O_DIRECTORY,
            Mode::empty(),
        )
        .map_err(std::io::Error::from)?;

        ensure_is_btrfs(&new_parent_fd)?;

        let args = ioctl::vol_args_v2 {
            id: ioctl::vol_args_v2_spec {
                name: name_bytes(path.as_ref().file_name().ok_or(Error::CannotCreateRoot)?),
            },
            fd: self.fd.as_raw_fd() as u64,
            flags: flags.bits(),
            ..Default::default()
        };
        unsafe {
            ioctl::snap_create_v2(new_parent_fd.as_raw_fd(), &args)
                .map_err(std::io::Error::from)?;
        }

        Self::open(path)
    }

    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn set_readonly(&mut self, ro: bool) -> Result<()> {
        let mut original_flags = 0;
        unsafe {
            ioctl::get_flags(self.fd.as_raw_fd(), &mut original_flags)
                .map_err(std::io::Error::from)?;
        }
        let flags = match ro {
            true => original_flags | SubvolFlags::READ_ONLY.bits(),
            false => original_flags & !SubvolFlags::READ_ONLY.bits(),
        };
        if flags != original_flags {
            trace!("setting flags: {flags}");
            unsafe {
                ioctl::set_flags(self.fd.as_raw_fd(), &flags).map_err(std::io::Error::from)?;
            }
        }
        Ok(())
    }

    fn name_bytes<const L: usize>(&self) -> [u8; L] {
        match self.opened_path.file_name() {
            None => [0; L],
            Some(name) => name_bytes(name),
        }
    }

    pub fn delete(self) -> std::result::Result<(), (Self, Error)> {
        let span = trace_span!("delete", path = self.path().display().to_string());
        let _enter = span.enter();
        match &self.parent {
            None => Err((self, Error::CannotDeleteRoot)),
            Some(parent) => {
                trace!("trying snap_destroy_v2");
                match unsafe {
                    ioctl::snap_destroy_v2(
                        parent.as_raw_fd(),
                        &ioctl::vol_args_v2 {
                            flags: ioctl::SPEC_BY_ID,
                            id: ioctl::vol_args_v2_spec { subvolid: self.id },
                            ..Default::default()
                        },
                    )
                } {
                    Ok(_) => Ok(()),
                    Err(e) => match e {
                        Errno::EOPNOTSUPP | Errno::ENOSYS => {
                            trace!("snap_destroy_v2 unsupported, trying snap_destroy");
                            match unsafe {
                                ioctl::snap_destroy(
                                    parent.as_raw_fd(),
                                    &ioctl::vol_args {
                                        fd: 0,
                                        name: self.name_bytes(),
                                    },
                                )
                            } {
                                Ok(_) => Ok(()),
                                Err(e) => Err((self, std::io::Error::from(e).into())),
                            }
                        }
                        _ => Err((self, std::io::Error::from(e).into())),
                    },
                }
            }
        }
    }

    pub fn info(&self) -> Result<Info> {
        let mut args = Default::default();
        unsafe {
            ioctl::get_subvol_info(self.fd.as_raw_fd(), &mut args).map_err(std::io::Error::from)?;
        }
        Ok(Info(args))
    }
}

#[derive(Clone, Copy)]
pub struct Info(ioctl::get_subvol_info_args);

impl Info {
    pub fn id(&self) -> u64 {
        self.0.id
    }

    pub fn uuid(&self) -> Uuid {
        Uuid::from_slice(&self.0.uuid).expect("always correct len")
    }

    pub fn parent_uuid(&self) -> Option<Uuid> {
        let uuid = Uuid::from_slice(&self.0.parent_uuid).expect("always correct len");
        match uuid.is_nil() {
            true => None,
            false => Some(uuid),
        }
    }

    pub fn ctransid(&self) -> u64 {
        self.0.ctransid
    }
}

impl Debug for Info {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Info")
            .field("id", &self.id())
            .field("uuid", &self.uuid())
            .field("parent_uuid", &self.parent_uuid())
            .finish_non_exhaustive()
    }
}

pub fn ensure_path_is_on_btrfs(path: impl AsRef<Path>) -> Result<()> {
    let fd = OpenOptions::new().read(true).open(path)?;
    ensure_is_btrfs(&fd)
}

#[cfg(test)]
#[allow(non_upper_case_globals)]
mod tests {
    #[cfg(not(unprivileged))]
    use std::collections::HashMap;
    use std::io::ErrorKind;
    use std::os::linux::fs::MetadataExt;
    #[cfg(not(unprivileged))]
    use std::process::Command;

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
        std::fs::create_dir("/work/foo").expect("dir creation failed");
        std::fs::create_dir("/work/foo/bar").expect("subdir creation failed");
        assert!(
            matches!(Subvolume::open("/work/foo/bar"), Err(Error::NotSubvol)),
            "expected error on subvol lookup for regular directory"
        );
        assert!(
            matches!(Subvolume::open("/tmp"), Err(Error::NotBtrfs)),
            "expected error on subvol lookup for non-btrfs"
        );
        Ok(())
    }

    #[test]
    fn create() {
        Subvolume::create("/work/foo").expect("failed to create subvol /work/foo");
        assert_eq!(
            std::fs::metadata("/work/foo")
                .expect("failed to stat /work/foo")
                .st_ino(),
            INO_SUBVOL,
            "subvol stat did not return expected inode number"
        )
    }

    #[test]
    fn toggle_readonly() {
        let mut subvol = Subvolume::create("/work/foo").expect("failed to create subvol /work/foo");
        std::fs::write("/work/foo/bar", "bar").expect("failed to write /work/foo/bar");
        subvol.set_readonly(true).expect("failed to set readonly");
        assert_eq!(
            std::fs::write("/work/foo/baz", "baz")
                .expect_err("should have failed to write /foo/baz")
                .kind(),
            ErrorKind::ReadOnlyFilesystem,
        );
        subvol.set_readonly(false).expect("failed to set readwrite");
        std::fs::write("/work/foo/qux", "qux").expect("failed to write /work/foo/qux");
    }

    #[test]
    fn snapshot() -> Result<()> {
        let subvol = Subvolume::create("/work/src")?;
        std::fs::write("/work/src/empty", "")?;
        let snap = subvol.snapshot("/work/snapshot", SnapshotFlags::empty())?;
        assert_eq!(snap.path(), Path::new("/work/snapshot"));
        assert!(snap.path().join("empty").exists());
        Ok(())
    }

    #[test]
    fn snapshot_readonly() {
        let subvol = Subvolume::create("/work/src").expect("failed to create src subvol");
        std::fs::write("/work/src/empty", "").expect("failed to write empty file");
        subvol
            .snapshot("/work/snapshot", SnapshotFlags::READONLY)
            .expect("failed to make snapshot");
        assert_eq!(
            std::fs::write("/work/snapshot/foo", "foo")
                .expect_err("should have failed to write /work/snapshot/foo")
                .kind(),
            ErrorKind::ReadOnlyFilesystem,
        );
    }

    // TODO(T167555826): re-enable test when user_subvol_rm_allowed is enabled
    #[cfg_attr(not(unprivileged), test)]
    #[cfg_attr(unprivileged, allow(dead_code))]
    fn delete() -> Result<()> {
        let subvol = Subvolume::create("/work/src").expect("failed to create src subvol");
        std::fs::write("/work/src/empty", "").expect("failed to write empty file");
        let snap = subvol.snapshot("/work/snapshot", SnapshotFlags::empty())?;
        assert_eq!(snap.path(), Path::new("/work/snapshot"));
        assert!(snap.path().join("empty").exists());
        snap.delete()
            .map_err(|(_, e)| e)
            .expect("failed to delete subvol");
        assert!(!Path::new("/work/snapshot").exists());
        Ok(())
    }

    #[test]
    fn get_info() {
        let subvol = Subvolume::open("/").expect("failed to open /");
        let info = subvol.info().expect("failed to get info");

        #[cfg(unprivileged)]
        {
            // in unprivileged contexts, 'btrfs subvolume show' will fail, so we
            // can't compare against the ground truth
            assert_ne!(info.id(), 0);
            assert!(!info.uuid().is_nil());
        }
        #[cfg(not(unprivileged))]
        {
            let out = Command::new("btrfs")
                .arg("subvolume")
                .arg("show")
                .arg("/")
                .output()
                .expect("failed to run btrfs");
            assert!(out.status.success(), "btrfs subvolume show failed");
            let stdout = String::from_utf8(out.stdout).expect("invalid utf8");
            let props: HashMap<_, _> = stdout
                .lines()
                .filter_map(|line| {
                    line.split_once(':')
                        .map(|(key, val)| (key.trim(), val.trim()))
                })
                .collect();
            assert_eq!(
                info.id(),
                props["Subvolume ID"].parse::<u64>().expect("bad cli id")
            );
            assert_eq!(
                info.uuid(),
                props["UUID"].parse::<Uuid>().expect("bad cli uuid")
            );
        }
    }
}
