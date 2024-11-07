/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::fd::AsRawFd as _;

use cap_std::fs::File;
use cap_std::fs::MetadataExt;

use super::maybe_chmod;
use super::maybe_chown;
use super::maybe_set_times;
use super::sanitize_mode;
use super::xattr_ops;
use crate::Contents;
use crate::Error;
use crate::Operation;
use crate::Result;

pub(super) fn compare<C: Contents>(old: File, new: File) -> Result<Vec<Operation<C>>> {
    let old_meta = old.metadata()?;
    let new_meta = new.metadata()?;
    let mut ops = Vec::new();

    // If they are both regular files, check for content changes
    if old_meta.file_type().is_file() && new_meta.file_type().is_file() {
        // re-open them for reading (the given fds are just O_PATH)
        let new_fd = std::fs::File::open(format!("/proc/self/fd/{}", new.as_raw_fd()))?;
        let old_fd = std::fs::File::open(format!("/proc/self/fd/{}", old.as_raw_fd()))?;
        let mut new_contents = C::from_file(new_fd)?;
        let mut old_contents = C::from_file(old_fd)?;
        if new_contents.differs(&mut old_contents)? {
            ops.push(Operation::Contents {
                contents: new_contents,
            });
        }
    }

    if new_meta.file_type().is_symlink() {
        let old_target = symlink_target(&old)?;
        let new_target = symlink_target(&new)?;
        if old_target != new_target {
            ops.push(Operation::Unlink);
            ops.push(Operation::Symlink { target: new_target });
        }
    }

    ops.extend(xattr_ops(Some(old), new)?);
    if let Some(op) = maybe_chmod(&old_meta, &new_meta) {
        ops.push(op);
    }
    if let Some(op) = maybe_chown(&old_meta, &new_meta) {
        ops.push(op);
    }
    if let Some(op) = maybe_set_times(&old_meta, &new_meta) {
        ops.push(op);
    }

    Ok(ops)
}

pub(super) fn add<C: Contents>(file: File) -> Result<Vec<Operation<C>>> {
    let meta = file.metadata()?;
    let ft = meta.file_type();
    let mut ops = vec![];

    if ft.is_file() {
        ops.push(Operation::Create {
            mode: sanitize_mode(meta.mode()),
        });
    } else if ft.is_symlink() {
        let target = symlink_target(&file)?;
        ops.push(Operation::Symlink { target });
    } else {
        let path = std::fs::read_link(format!("/proc/self/fd/{}", file.as_raw_fd()))?;
        return Err(Error::UnsupportedFileType(path, ft));
    }

    // Only add the contents if it's a regular file
    if ft.is_file() {
        let f_for_read = std::fs::File::open(format!("/proc/self/fd/{}", file.as_raw_fd()))?;
        ops.push(Operation::Contents {
            contents: C::from_file(f_for_read)?,
        });
    }

    ops.extend(xattr_ops(None, file)?);

    ops.push(Operation::Chown {
        uid: meta.uid(),
        gid: meta.gid(),
    });
    ops.push(Operation::SetTimes {
        mtime: meta.modified()?.into_std(),
        atime: meta.accessed()?.into_std(),
    });

    Ok(ops)
}

fn symlink_target(f: &File) -> std::io::Result<std::path::PathBuf> {
    let path = std::fs::read_link(format!("/proc/self/fd/{}", f.as_raw_fd()))?;
    std::fs::read_link(&path)
}
