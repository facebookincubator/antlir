/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use clap::Parser;

mod container;
mod exec;
mod runtime;
mod shell_help;
mod spawn;
mod spawn_common;

#[derive(Parser, Debug)]
enum Args {
    /// Spawn a container to run the test
    Spawn(spawn::Args),
    /// Execute the test from inside the container
    Exec(exec::Args),
    ShellHelp(shell_help::Args),
    /// Run an interactive shell inside the test container
    Container(container::Args),
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .init();

    let args = Args::parse();

    if let Err(e) = match args {
        Args::Spawn(a) => a.run(),
        Args::Exec(a) => a.run(),
        Args::ShellHelp(a) => a.run(),
        Args::Container(a) => a.run(),
    } {
        eprintln!("{e:#}");
        Err(e)
    } else {
        Ok(())
    }
}
