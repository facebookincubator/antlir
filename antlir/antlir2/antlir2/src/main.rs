/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(iter_intersperse)]

use std::process::ExitStatus;

use clap::Parser;
use thiserror::Error;
use tracing_subscriber::prelude::*;

mod cmd;

#[derive(Debug, Error)]
pub enum Error {
    #[error(transparent)]
    Compile(#[from] antlir2_compile::Error),
    #[error(transparent)]
    Depgraph(#[from] antlir2_depgraph::Error<'static>),
    #[error("subprocess exited with {0}")]
    Subprocess(ExitStatus),
    #[error("{0:#?}")]
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
    Compile(cmd::Compile),
    Depgraph(cmd::Depgraph),
    Map(cmd::Map),
    Plan(cmd::Plan),
    Shell(cmd::Shell),
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
        Subcommand::Compile(x) => x.run(),
        Subcommand::Depgraph(p) => p.run(),
        Subcommand::Map(x) => x.run(),
        Subcommand::Plan(x) => x.run(),
        Subcommand::Shell(x) => x.run(),
    };
    if let Err(e) = result {
        tracing::error!("{e}");
        eprintln!("{e:?}");
        std::process::exit(1);
    }
}
