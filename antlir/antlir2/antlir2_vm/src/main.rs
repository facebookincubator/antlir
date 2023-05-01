/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod isolation;
mod utils;

use std::env;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use tracing::debug;

use crate::isolation::is_isolated;
use crate::isolation::isolated;
use crate::isolation::Platform;
use crate::utils::log_command;

type Result<T> = std::result::Result<T, anyhow::Error>;

#[derive(Debug, Parser)]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Debug, Subcommand)]
enum Commands {
    /// Respawn inside isolated image and execute `Run` command
    Isolate(IsolateArgs),
    /// Run the VM. Must be executed inside container.
    Run(RunArgs),
}

#[derive(Debug, Args)]
struct RunArgs {
    /// Json-encoded string for VM configuration
    #[arg(long)]
    vm_spec: String,
    /// Json-encoded string describing paths of binary and data required by VM
    #[arg(long)]
    runtime: String,
}

#[derive(Debug, Args)]
struct IsolateArgs {
    /// Path to container image. VM will be spawned inside the container.
    #[arg(long)]
    image: String,
    /// List of env variable names to pass through into the container.
    #[arg(long)]
    envs: Option<Vec<String>>,
    #[command(flatten)]
    vm_args: RunArgs,
}

fn respawn(args: &IsolateArgs) -> Result<()> {
    let isolated = isolated(
        PathBuf::from(&args.image),
        &args.envs.clone().unwrap_or_default(),
    )?;
    let exe = env::current_exe().context("while getting argv[0]")?;
    log_command(
        isolated
            .into_command()
            .arg(&exe)
            .arg("run")
            .arg("--vm-spec")
            .arg(&args.vm_args.vm_spec)
            .arg("--runtime")
            .arg(&args.vm_args.runtime),
    )
    .status()?;
    Ok(())
}

fn run(args: &RunArgs) -> Result<()> {
    if !is_isolated()? {
        return Err(anyhow!("run must be called from inside container"));
    }
    // TODO: call vm run in next patch
    println!("Run command with args: {:?}", args);
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();
    Platform::set()?;

    debug!("Args: {:?}", env::args());

    let cli = Cli::parse();
    match &cli.command {
        Commands::Isolate(args) => respawn(args),
        Commands::Run(args) => run(args),
    }
}
