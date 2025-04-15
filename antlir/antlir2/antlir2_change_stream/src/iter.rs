/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;

use cap_std::fs::Dir;

use crate::Change;
use crate::Contents;
use crate::Result;
use crate::compare;

pub struct Iter<C> {
    rx: std::sync::mpsc::IntoIter<Result<Change<C>>>,
}

impl<C: Contents + 'static> Iter<C> {
    /// Diff two filesystem trees and produce a change stream that can be used
    /// to convert `old` to `new`.
    pub fn diff(old: impl AsRef<Path>, new: impl AsRef<Path>) -> Result<Self> {
        let old = Dir::open_ambient_dir(old.as_ref(), cap_std::ambient_authority())?;
        let new = Dir::open_ambient_dir(new.as_ref(), cap_std::ambient_authority())?;
        Self::with_initial_instruction(compare::Instruction::CompareTree {
            prefix: "".into(),
            old,
            new,
        })
    }

    /// Generate a change stream for a completely new directory.
    pub fn from_empty(new: impl AsRef<Path>) -> Result<Self> {
        let new = Dir::open_ambient_dir(new.as_ref(), cap_std::ambient_authority())?;
        Self::with_initial_instruction(compare::Instruction::AddTree {
            prefix: "".into(),
            dir: new,
        })
    }

    fn with_initial_instruction(instruction: compare::Instruction<C>) -> Result<Self> {
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::Builder::new()
            .name("compare".to_owned())
            .spawn(move || {
                if let Err(e) = compare::run_to_completion::<C, _>(vec![instruction], |change| {
                    tx.send(Ok(change))
                        .expect("failed to send change on channel");
                }) {
                    tx.send(Err(e)).expect("failed to send");
                }
            })?;
        Ok(Self { rx: rx.into_iter() })
    }
}

impl<C: Contents> Iterator for Iter<C> {
    type Item = Result<Change<C>>;

    fn next(&mut self) -> Option<Self::Item> {
        self.rx.next()
    }
}
