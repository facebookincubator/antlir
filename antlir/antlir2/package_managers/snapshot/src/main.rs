/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::path::PathBuf;
use std::process::ExitCode;
use std::sync::Mutex;

use anyhow::Result;
use clap::Parser;
use tracing::error;
use tracing_subscriber::prelude::*;

mod checksums;
mod decompress;
mod generate;
mod parse;
mod snapshot;

#[derive(Debug, Parser)]
struct Args {
    #[clap(short, long, default_value_t=1, action = clap::ArgAction::Count)]
    verbose: u8,
    #[clap(subcommand)]
    sub: Sub,
    #[clap(long)]
    log: Option<PathBuf>,
}

#[derive(Debug, Parser)]
enum Sub {
    Decompress(decompress::Decompress),
    Generate(generate::Generate),
    Parse(parse::Parse),
    Snapshot(snapshot::Snapshot),
}

#[fbinit::main]
async fn main(fb: fbinit::FacebookInit) -> ExitCode {
    // Wrap the real main function with this so that we can print out the full
    // error
    if let Err(e) = do_main(fb).await {
        error!("{e:#?}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

async fn do_main(fb: fbinit::FacebookInit) -> Result<()> {
    let args = Args::parse();
    let stderr_level = match args.verbose {
        0 => tracing_subscriber::filter::LevelFilter::ERROR,
        1 => tracing_subscriber::filter::LevelFilter::WARN,
        2 => tracing_subscriber::filter::LevelFilter::INFO,
        3 => tracing_subscriber::filter::LevelFilter::DEBUG,
        _ => tracing_subscriber::filter::LevelFilter::TRACE,
    };
    let stderr_layer = tracing_subscriber::fmt::layer().with_filter(stderr_level);
    let file_layer = if let Some(log) = &args.log {
        let file = File::create(log)?;
        Some(
            tracing_subscriber::fmt::layer()
                .with_writer(Mutex::new(file))
                .with_filter(tracing_subscriber::filter::LevelFilter::DEBUG),
        )
    } else {
        None
    };
    tracing_subscriber::registry()
        .with(stderr_layer)
        .with(file_layer)
        .init();
    match args.sub {
        Sub::Decompress(sub) => sub.run(),
        Sub::Generate(sub) => sub.run(),
        Sub::Parse(sub) => sub.run(),
        Sub::Snapshot(sub) => sub.run(fb).await,
    }
}
