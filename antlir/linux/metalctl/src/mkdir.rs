/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{Context, Result};
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Opts {
    dir: String,
    #[structopt(short = "p")]
    parent: bool,
}

pub fn mkdir(opts: Opts) -> Result<()> {
    if opts.parent {
        return std::fs::create_dir_all(&opts.dir)
            .with_context(|| format!("failed to create_dir_all({})", opts.dir));
    }
    std::fs::create_dir(&opts.dir).with_context(|| format!("failed to create_dir({})", opts.dir))
}
