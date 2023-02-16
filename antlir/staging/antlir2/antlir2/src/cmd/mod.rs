/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod depgraph;
pub(crate) use depgraph::Depgraph;

pub(crate) trait Subcommand {
    fn run(self) -> crate::Result<()>;
}
