/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod compile;
mod depgraph;
mod map;
mod shell;
pub(crate) use compile::Compile;
pub(crate) use depgraph::Depgraph;
pub(crate) use map::Map;
pub(crate) use shell::Shell;

pub(crate) trait Subcommand {
    fn run(self) -> crate::Result<()>;
}
