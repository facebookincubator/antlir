/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use antlir2_working_volume::WorkingVolume;
use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
struct Args {
    #[clap(default_value = "antlir2-out")]
    working_dir: PathBuf,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let args = Args::parse();

    let working_volume = WorkingVolume::ensure(args.working_dir)?;
    working_volume.collect_garbage()?;
    Ok(())
}
