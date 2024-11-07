/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::fd::AsRawFd as _;
use std::path::Path;

use cap_std::fs::Dir;
use cap_std::fs::MetadataExt;
use cap_std::fs::OpenOptions;
use cap_std::fs::OpenOptionsExt;

use super::maybe_chmod;
use super::maybe_chown;
use super::maybe_set_times;
use super::sanitize_mode;
use super::xattr_ops;
use super::Instruction;
use crate::Change;
use crate::Operation;
use crate::Result;

fn path_open_opts() -> OpenOptions {
    let mut opts = OpenOptions::new();
    opts.read(true)
        .custom_flags(libc::O_PATH | libc::O_NOFOLLOW);
    opts
}

pub(super) fn compare<C>(prefix: &Path, old: Dir, new: Dir) -> Result<Vec<Instruction<C>>> {
    let mut stack: Vec<Instruction<C>> = Vec::new();
    // Add the comparison of the top-level directories to the bottom
    // of the stack so the changes are yielded after any inner
    // content changes
    let old_meta = old.dir_metadata()?;
    let new_meta = new.dir_metadata()?;
    if let Some(op) = maybe_chown(&old_meta, &new_meta) {
        stack.push(Instruction::Change(Change::new(prefix.to_owned(), op)));
    }
    if let Some(op) = maybe_chmod(&old_meta, &new_meta) {
        stack.push(Instruction::Change(Change::new(prefix.to_owned(), op)));
    }
    if let Some(op) = maybe_set_times(&old_meta, &new_meta) {
        stack.push(Instruction::Change(Change::new(prefix.to_owned(), op)));
    }
    stack.extend(
        xattr_ops(Some(old.as_raw_fd()), new.as_raw_fd())?
            .into_iter()
            .map(|op| Instruction::Change(Change::new(prefix.to_owned(), op))),
    );

    for entry in old.entries()? {
        let entry = entry?;
        let old_filetype = entry.file_type()?;
        let name = entry.file_name();
        if matches!(new.symlink_metadata(&name), Err(e) if e.kind() == std::io::ErrorKind::NotFound)
        {
            if old_filetype.is_dir() {
                stack.push(Instruction::RemoveTree {
                    prefix: prefix.join(name),
                    dir: entry.open_dir()?,
                });
            } else {
                stack.push(Instruction::Change(Change::new(
                    prefix.join(name),
                    Operation::Unlink,
                )));
            }
        }
    }

    for entry in new.entries()? {
        let entry = entry?;
        let new_meta = entry.metadata()?;
        let new_ft = new_meta.file_type();
        let name = entry.file_name();

        // There are two paths that require treating an entry as something
        // entirely new, so just compute the instruction once.
        // Being super pedantic, in the case that we *don't* treat this as
        // something new, this results in an unnecessary `open/close` pair of
        // syscalls, but it makes the code easier to read.
        let new_instruction = if new_ft.is_dir() {
            Instruction::AddTree {
                prefix: prefix.join(&name),
                dir: entry.open_dir()?,
            }
        } else {
            Instruction::NewFile {
                path: prefix.join(&name),
                file: entry.open_with(&path_open_opts())?,
            }
        };

        match old.symlink_metadata(&name) {
            Ok(old_meta) => {
                let old_ft = old_meta.file_type();
                if old_ft != new_ft {
                    // If the file type changed, we need to first delete it and
                    // then add it as something new
                    stack.push(new_instruction);
                    if old_ft.is_dir() {
                        stack.push(Instruction::RemoveTree {
                            prefix: prefix.join(&name),
                            dir: old.open_dir(&name)?,
                        });
                    } else {
                        stack.push(Instruction::Change(Change::new(
                            prefix.join(&name),
                            Operation::Unlink,
                        )));
                    }
                } else if new_ft.is_dir() {
                    stack.push(Instruction::CompareTree {
                        prefix: prefix.join(&name),
                        old: old.open_dir(&name)?,
                        new: entry.open_dir()?,
                    });
                } else {
                    stack.push(Instruction::CompareFile {
                        path: prefix.join(&name),
                        old: old.open_with(&name, &path_open_opts())?,
                        new: entry.open_with(&path_open_opts())?,
                    });
                }
            }
            // Does not exist in the old tree, this is a new file or directory
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                stack.push(new_instruction);
            }
            Err(e) => return Err(e.into()),
        }
    }

    Ok(stack)
}

pub(super) fn remove<C>(prefix: &Path, dir: Dir) -> Result<Vec<Instruction<C>>> {
    let mut stack: Vec<Instruction<C>> = Vec::new();
    // Add the rmdir to the bottom of the stack so that it happens
    // after all the internal deletions
    stack.push(Instruction::Change(Change::new(
        prefix.to_owned(),
        Operation::Rmdir,
    )));
    for entry in dir.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            stack.push(Instruction::RemoveTree {
                prefix: prefix.join(name),
                dir: entry.open_dir()?,
            });
        } else {
            stack.push(Instruction::Change(Change::new(
                prefix.join(name),
                Operation::Unlink,
            )));
        }
    }
    Ok(stack)
}

pub(super) fn add<C>(prefix: &Path, dir: Dir) -> Result<Vec<Instruction<C>>> {
    let mut stack: Vec<Instruction<C>> = Vec::new();
    let dir_meta = dir.dir_metadata()?;
    // Add the timestamps to the bottom of the stack so that it
    // happens last and no subsequent modifications will overwrite
    // it on the receive side.
    stack.push(Instruction::Change(Change::new(
        prefix.to_owned(),
        Operation::SetTimes {
            mtime: dir_meta.modified()?.into_std(),
            atime: dir_meta.accessed()?.into_std(),
        },
    )));
    stack.push(Instruction::Change(Change::new(
        prefix.to_owned(),
        Operation::Chown {
            uid: dir_meta.uid(),
            gid: dir_meta.gid(),
        },
    )));

    for entry in dir.entries()? {
        let entry = entry?;
        let name = entry.file_name();
        let ft = entry.file_type()?;
        if ft.is_dir() {
            stack.push(Instruction::AddTree {
                prefix: prefix.join(name),
                dir: entry.open_dir()?,
            });
        } else {
            stack.push(Instruction::NewFile {
                path: prefix.join(name),
                file: entry.open_with(&path_open_opts())?,
            });
        }
    }

    // Add the mkdir to the top of the stack so that it gets yielded before all
    // the internal additions
    stack.push(Instruction::Change(Change::new(
        prefix.to_owned(),
        Operation::Mkdir {
            mode: sanitize_mode(dir_meta.mode()),
        },
    )));
    Ok(stack)
}
