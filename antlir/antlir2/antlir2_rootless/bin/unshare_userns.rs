/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;

fn main() -> Result<()> {
    antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
    let mut args = std::env::args().skip(1).collect::<Vec<_>>();
    ensure!(!args.is_empty(), "need at least one arg to execute");
    let mut cmd = Command::new(args.remove(0));
    cmd.args(args);
    Err(cmd.exec().into())
}
