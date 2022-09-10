/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;

#[derive(Debug, PartialEq)]
pub(crate) enum PrimitiveState {
    Running,
    Aborted,
    Done,
}

pub trait BlockingSyncPrimitive {
    /// Stop the blocking primitive to terminate the application
    fn halt(&self, unplanned: bool) -> anyhow::Result<()>;
}
