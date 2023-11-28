/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(iter_intersperse)]

use std::fs::File;
use std::path::PathBuf;
use std::process::ExitStatus;

use anyhow::Context;
use clap::Parser;
use colored::Colorize;
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
    #[error(transparent)]
    Btrfs(#[from] antlir2_btrfs::Error),
    #[error(transparent)]
    Rootless(#[from] antlir2_rootless::Error),
    #[error("{0:#?}")]
    Uncategorized(#[from] anyhow::Error),
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Parser, Debug)]
struct Args {
    #[command(flatten)]
    log: LogArgs,
    #[clap(subcommand)]
    subcommand: Subcommand,
}

#[derive(clap::Args, Debug)]
struct LogArgs {
    #[clap(long)]
    /// File to write logs to in addition to stdout
    logs: Option<PathBuf>,
}

impl LogArgs {
    fn file(&self) -> anyhow::Result<Option<File>> {
        match &self.logs {
            Some(path) => {
                // This is not technically atomic, but does a good enough job to
                // clean up after previous buck builds. We know that buck isn't
                // going to run two concurrent processes with this same log file
                // at the same time, so we can ignore the small race between
                // removing this file and creating a new one, the end result
                // being that the log file is safely truncated.
                match std::fs::remove_file(path) {
                    Ok(()) => Ok(()),
                    Err(e) => match e.kind() {
                        std::io::ErrorKind::NotFound => Ok(()),
                        _ => Err(e),
                    },
                }?;
                Ok(Some(
                    std::fs::OpenOptions::new()
                        .create(true)
                        .append(true)
                        .open(path)
                        .context("while opening new logs file")?,
                ))
            }
            None => Ok(None),
        }
    }
}

#[derive(Parser, Debug)]
enum Subcommand {
    Compile(cmd::Compile),
    Depgraph(cmd::Depgraph),
    Map(cmd::Map),
    Plan(cmd::Plan),
    Shell(cmd::Shell),
}

fn main() -> Result<()> {
    let args = Args::parse();

    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::Layer::default().with_ansi(false))
        .with(args.log.file()?.map(|file| {
            tracing_subscriber::fmt::Layer::default()
                .with_ansi(false)
                .with_writer(file)
        }))
        .init();

    let rootless = antlir2_rootless::init().context("while setting up antlir2_rootless")?;
    let result = match args.subcommand {
        Subcommand::Compile(x) => x.run(),
        Subcommand::Depgraph(x) => x.run(rootless),
        Subcommand::Map(x) => x.run(rootless),
        Subcommand::Plan(x) => x.run(),
        Subcommand::Shell(x) => x.run(),
    };
    if let Err(e) = result {
        eprintln!("{}", format!("{e:#?}").red());
        eprintln!("{}", e.to_string().red());
        std::process::exit(1);
    }
    Ok(())
}
