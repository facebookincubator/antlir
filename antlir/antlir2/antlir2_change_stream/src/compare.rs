/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use cap_std::fs::Dir;
use cap_std::fs::File;
use cap_std::fs::Metadata;
use cap_std::fs::MetadataExt;

use crate::Change;
use crate::Contents;
use crate::Operation;
use crate::Result;

mod file;
mod tree;
mod xattrs;
use xattrs::xattr_ops;

/// Control is an instruction that is placed on a stack
#[derive(Debug)]
pub(crate) enum Instruction<C> {
    /// Immediately yield this change
    Change(Change<C>),
    /// Compare two directory trees
    CompareTree { prefix: PathBuf, old: Dir, new: Dir },
    /// Recursively remove a directory, contents first
    RemoveTree { prefix: PathBuf, dir: Dir },
    /// Recursively add an entirely new tree
    AddTree { prefix: PathBuf, dir: Dir },
    /// Compare two file entries (anything other than a directory, they may be
    /// symlinks, device nodes, regular files, etc)
    CompareFile { path: PathBuf, old: File, new: File },
    /// Add a new file entry
    NewFile { path: PathBuf, file: File },
}

fn maybe_chown<C>(old: &Metadata, new: &Metadata) -> Option<Operation<C>> {
    if old.uid() != new.uid() || old.gid() != new.gid() {
        Some(Operation::Chown {
            uid: new.uid(),
            gid: new.gid(),
        })
    } else {
        None
    }
}

fn maybe_chmod<C>(old: &Metadata, new: &Metadata) -> Option<Operation<C>> {
    if sanitize_mode(old.mode()) != sanitize_mode(new.mode()) {
        Some(Operation::Chmod {
            mode: sanitize_mode(new.mode()),
        })
    } else {
        None
    }
}

fn maybe_set_times<C>(old: &Metadata, new: &Metadata) -> Option<Operation<C>> {
    match (old.modified(), new.modified(), new.accessed()) {
        (Ok(old), Ok(new_mt), Ok(new_at)) if old != new_mt => Some(Operation::SetTimes {
            mtime: new_mt.into_std(),
            atime: new_at.into_std(),
        }),
        _ => None,
    }
}

/// Remove the file type bits, but keep the mode and permissions bits
fn sanitize_mode(mode: u32) -> u32 {
    mode & !0o0170000
}

/// Run the stack machine to completion using this starting set of instructions,
/// yielding each change as it is produced by the stack machine.
pub(crate) fn run_to_completion<C, F>(mut stack: Vec<Instruction<C>>, mut yield_fn: F) -> Result<()>
where
    C: Contents,
    F: FnMut(Change<C>),
{
    while let Some(instr) = stack.pop() {
        match instr {
            Instruction::Change(c) => yield_fn(c),
            Instruction::CompareTree { prefix, old, new } => {
                stack.extend(tree::compare(&prefix, old, new)?);
            }
            Instruction::RemoveTree { prefix, dir } => {
                stack.extend(tree::remove(&prefix, dir)?);
            }
            Instruction::AddTree { prefix, dir } => {
                stack.extend(tree::add(&prefix, dir)?);
            }
            Instruction::CompareFile { path, old, new } => {
                let ops = file::compare(old, new)?;
                stack.extend(
                    ops.into_iter()
                        .rev()
                        .map(|op| Instruction::Change(Change::new(path.clone(), op))),
                );
            }
            Instruction::NewFile { path, file } => {
                let ops = file::add(file)?;
                stack.extend(
                    ops.into_iter()
                        .rev()
                        .map(|op| Instruction::Change(Change::new(path.clone(), op))),
                );
            }
        }
    }
    Ok(())
}
