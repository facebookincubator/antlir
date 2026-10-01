/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::Ordering;
use std::time::SystemTime;

use cap_std::fs::FileType;
use serde::Deserialize;
use serde::Serialize;

mod compare;
pub mod contents;
mod iter;

pub use contents::Contents;
pub use iter::Iter;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("cannot handle an entry {0} with file type {1:?}")]
    UnsupportedFileType(PathBuf, FileType),
    #[error(transparent)]
    Walkdir(#[from] walkdir::Error),
    #[error(transparent)]
    Io(#[from] std::io::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

/// Whether [`Iter::diff_with`] may skip reading regular files that a btrfs
/// snapshot proves unchanged.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum FastSnapshotDiff {
    /// Read and compare every regular file that exists on both sides.
    #[default]
    Off,
    /// Skip reading files whose inode, ctime, size and physical extents match
    /// across a read-only snapshot.
    On,
    /// Like `On`, but byte-compare every skipped file. A file that differs is
    /// read in full and counted as mismatched instead of failing the diff,
    /// so this validates correctness without speeding anything up.
    VerifyAll,
}

/// How regular files that exist on both sides of a diff were compared.
#[derive(Debug)]
pub struct Stats {
    fast_snapshot_diff: bool,
    skipped: AtomicU64,
    verified: AtomicU64,
    mismatched: AtomicU64,
    read: AtomicU64,
}

impl Stats {
    fn new(fast_snapshot_diff: bool) -> Self {
        Self {
            fast_snapshot_diff,
            skipped: AtomicU64::new(0),
            verified: AtomicU64::new(0),
            mismatched: AtomicU64::new(0),
            read: AtomicU64::new(0),
        }
    }

    /// Whether the fast snapshot diff was requested and the trees qualified.
    pub fn fast_snapshot_diff(&self) -> bool {
        self.fast_snapshot_diff
    }

    /// Files not read because they are provably unchanged.
    pub fn skipped(&self) -> u64 {
        self.skipped.load(Ordering::Relaxed)
    }

    /// Skipped files that were byte-compared anyway.
    pub fn verified(&self) -> u64 {
        self.verified.load(Ordering::Relaxed)
    }

    /// Verified files whose bytes differed, which were read in full instead.
    pub fn mismatched(&self) -> u64 {
        self.mismatched.load(Ordering::Relaxed)
    }

    /// Files read and compared in full.
    pub fn read(&self) -> u64 {
        self.read.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Operation<C> {
    /// Change the mode bits of a file or directory
    Chmod { mode: u32 },
    /// Change the owner and group of a file or directory
    Chown { uid: u32, gid: u32 },
    /// Set the entire contents of a file.
    Contents { contents: C },
    /// Create a new regular file
    Create { mode: u32 },
    /// Create a symlink with the given target
    Symlink { target: PathBuf },
    /// Create a hardlink to the given target
    HardLink { target: PathBuf },
    /// Create a new empty directory
    Mkdir { mode: u32 },
    /// Create a new fifo
    Mkfifo { mode: u32 },
    /// Create a new device node
    Mknod { rdev: u64, mode: u32 },
    /// Remove an empty directory
    Rmdir,
    /// Remove a file
    Unlink,
    /// Rename a file or directoy
    Rename { to: PathBuf },
    /// Set timestamps on the file
    SetTimes {
        atime: SystemTime,
        mtime: SystemTime,
    },
    /// Set an xattr
    SetXattr { name: OsString, value: Vec<u8> },
    /// Remove an xattr
    RemoveXattr { name: OsString },
    /// Done with a file or directory, no more changes will be emitted for it
    Close,
}

#[derive(Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Change<C> {
    path: PathBuf,
    operation: Operation<C>,
}

impl<C> Change<C> {
    pub fn new(path: PathBuf, operation: Operation<C>) -> Self {
        Self { path, operation }
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn operation(&self) -> &Operation<C> {
        &self.operation
    }

    pub fn into_operation(self) -> Operation<C> {
        self.operation
    }
}
