/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use clap::Parser;
use thiserror::Error;
use tracing_subscriber::prelude::*;

mod cmd;
use cmd::Subcommand as _;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Depgraph(#[from] antlir2_depgraph::Error<'static>),
    #[error(transparent)]
    Uncategorized(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Parser, Debug)]
struct Args {
    #[clap(subcommand)]
    subcommand: Subcommand,
}

#[derive(Parser, Debug)]
enum Subcommand {
    Depgraph(cmd::Depgraph),
}

fn main() {
    let args = Args::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::Layer::default()
                .event_format(
                    tracing_glog::Glog::default()
                        .with_span_context(true)
                        .with_timer(tracing_glog::LocalTime::default()),
                )
                .fmt_fields(tracing_glog::GlogFields::default()),
        )
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let result = match args.subcommand {
        Subcommand::Depgraph(p) => p.run(),
    };
    if let Err(e) = result {
        tracing::error!("{e}");
        eprintln!("{e:#?}");
        std::process::exit(1);
    }
}
