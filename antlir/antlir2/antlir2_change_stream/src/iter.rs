/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::sync::Arc;

use cap_std::fs::Dir;

use crate::Change;
use crate::Contents;
use crate::FastSnapshotDiff;
use crate::Result;
use crate::Stats;
use crate::compare;
use crate::compare::FileComparison;
use crate::compare::Shortcut;
use crate::compare::Snapshot;

pub struct Iter<C> {
    rx: std::sync::mpsc::IntoIter<Result<Change<C>>>,
    stats: Arc<Stats>,
}

impl<C: Contents + 'static> Iter<C> {
    /// Diff two filesystem trees and produce a change stream that can be used
    /// to convert `old` to `new`.
    pub fn diff(old: impl AsRef<Path>, new: impl AsRef<Path>) -> Result<Self> {
        Self::diff_with(old, new, FastSnapshotDiff::Off)
    }

    /// Like [`Iter::diff`], optionally skipping files that a btrfs snapshot
    /// proves unchanged. If `new` is not a read-only snapshot of a read-only
    /// `old`, every file is read as usual.
    pub fn diff_with(
        old: impl AsRef<Path>,
        new: impl AsRef<Path>,
        fast_snapshot_diff: FastSnapshotDiff,
    ) -> Result<Self> {
        let old_dir = Dir::open_ambient_dir(old.as_ref(), cap_std::ambient_authority())?;
        let new_dir = Dir::open_ambient_dir(new.as_ref(), cap_std::ambient_authority())?;
        let verify_all = match fast_snapshot_diff {
            FastSnapshotDiff::Off => None,
            FastSnapshotDiff::On => Some(false),
            FastSnapshotDiff::VerifyAll => Some(true),
        };
        let shortcut = verify_all.and_then(|verify_all| {
            Snapshot::detect(&old_dir, &new_dir)
                .inspect_err(|e| {
                    tracing::info!(
                        "fast snapshot diff unavailable, reading every file: {} is not a read-only snapshot of {}: {e}",
                        new.as_ref().display(),
                        old.as_ref().display(),
                    )
                })
                .ok()
                .map(|snapshot| Shortcut::new(snapshot, verify_all))
        });
        Self::with_initial_instruction(
            compare::Instruction::CompareTree {
                prefix: "".into(),
                old: old_dir,
                new: new_dir,
            },
            shortcut,
        )
    }

    /// Generate a change stream for a completely new directory.
    pub fn from_empty(new: impl AsRef<Path>) -> Result<Self> {
        let new = Dir::open_ambient_dir(new.as_ref(), cap_std::ambient_authority())?;
        Self::with_initial_instruction(
            compare::Instruction::AddTree {
                prefix: "".into(),
                dir: new,
            },
            None,
        )
    }

    fn with_initial_instruction(
        instruction: compare::Instruction<C>,
        shortcut: Option<Shortcut>,
    ) -> Result<Self> {
        let stats = Arc::new(Stats::new(shortcut.is_some()));
        let files = FileComparison {
            shortcut,
            stats: stats.clone(),
        };
        // use a bounded channel to limit the number of NewFile Changes (open file descriptors) that can be created at one time
        let (tx, rx) = std::sync::mpsc::sync_channel(4096);
        std::thread::Builder::new()
            .name("compare".to_owned())
            .spawn(move || {
                if let Err(e) =
                    compare::run_to_completion::<C, _>(vec![instruction], &files, |change| {
                        tx.send(Ok(change))
                            .expect("failed to send change on channel");
                    })
                {
                    tx.send(Err(e)).expect("failed to send");
                }
            })?;
        Ok(Self {
            rx: rx.into_iter(),
            stats,
        })
    }

    /// Counters for how regular files were compared. They keep updating until
    /// the iterator is exhausted, so take this before consuming it.
    pub fn stats(&self) -> Arc<Stats> {
        self.stats.clone()
    }
}

impl<C: Contents> Iterator for Iter<C> {
    type Item = Result<Change<C>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.rx.next()
    }
}
