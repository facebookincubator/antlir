/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

//! CLI to extract named files from btrfs send-stream(s) in userspace, without
//! `btrfs receive`. The reconstruction logic lives in the [`extract`] module.

mod extract;

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use clap::Parser;
use tokio::io::BufReader;

use crate::extract::Extractor;

#[derive(Parser)]
#[clap(about = "Extract named files from btrfs send-stream(s) without `btrfs receive`")]
struct Args {
    /// Directory to materialize extracted files into; in-stream relative paths
    /// are preserved beneath it.
    #[clap(long)]
    output_dir: PathBuf,

    /// File base name to extract, matched against each in-stream path's final
    /// component. May be repeated.
    #[clap(long = "name", required = true)]
    names: Vec<String>,

    /// Send-stream files, applied in order: the full base subvolume first, then
    /// any incremental layers stacked on top of it.
    #[clap(required = true)]
    sendstreams: Vec<PathBuf>,
}

#[tokio::main]
async fn main() -> Result<()> {
    let args = Args::parse();
    let mut extractor = Extractor::new(&args.output_dir, args.names.clone());
    for path in &args.sendstreams {
        let file = tokio::fs::File::open(path)
            .await
            .with_context(|| format!("opening send-stream {}", path.display()))?;
        extractor
            .extract(BufReader::new(file))
            .await
            .with_context(|| format!("extracting from {}", path.display()))?;
    }
    let extracted = extractor.finish().await?;
    ensure!(
        extracted > 0,
        "no files matching {:?} were found in the send-stream(s)",
        args.names
    );
    Ok(())
}
