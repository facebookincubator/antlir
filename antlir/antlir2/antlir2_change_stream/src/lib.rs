/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
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
}
