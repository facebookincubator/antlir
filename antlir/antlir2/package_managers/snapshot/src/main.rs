/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::process::ExitCode;

use anyhow::Result;
use clap::Parser;
use tracing::error;
use tracing_subscriber::prelude::*;

mod checksums;
mod decompress;
mod parse;

#[derive(Debug, Parser)]
struct Args {
    #[clap(short, long, default_value_t=1, action = clap::ArgAction::Count)]
    verbose: u8,
    #[clap(subcommand)]
    sub: Sub,
}

#[derive(Debug, Parser)]
enum Sub {
    Decompress(decompress::Decompress),
    Parse(parse::Parse),
}

fn main() -> ExitCode {
    // Wrap the real main function with this so that we can print out the full
    // error
    if let Err(e) = do_main() {
        error!("{e:#?}");
        return ExitCode::FAILURE;
    }
    ExitCode::SUCCESS
}

fn do_main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::FmtSubscriber::builder()
        .with_max_level(match args.verbose {
            0 => tracing::Level::ERROR,
            1 => tracing::Level::WARN,
            2 => tracing::Level::INFO,
            3 => tracing::Level::DEBUG,
            _ => tracing::Level::TRACE,
        })
        .finish()
        .init();

    match args.sub {
        Sub::Decompress(sub) => sub.run(),
        Sub::Parse(sub) => sub.run(),
    }
}
