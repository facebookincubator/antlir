/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use clap::Parser;
use futures::StreamExt;
use sendstream_parser::wire;
use tokio::io::BufReader;

#[derive(Parser)]
struct Args {
    /// Path to a sendstream file, or "-" to read from stdin
    sendstream: PathBuf,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args = Args::parse();
    let file = stdio_path::open(&args.sendstream)?;
    let file = BufReader::new(tokio::fs::File::from_std(file));
    let mut stream = wire::parse(file);
    while let Some(cmd) = stream.next().await {
        let cmd = cmd?;
        println!("{cmd:#?}");
    }
    Ok(())
}
