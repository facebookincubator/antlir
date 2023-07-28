/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod disk;
mod isolation;
mod net;
mod runtime;
mod share;
mod ssh;
mod types;
mod utils;
mod vm;

use std::env;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use json_arg::JsonFile;
use tracing::debug;

use crate::isolation::is_isolated;
use crate::isolation::isolated;
use crate::isolation::Platform;
use crate::runtime::set_runtime;
use crate::types::MachineOpts;
use crate::types::RuntimeOpts;
use crate::types::VMArgs;
use crate::utils::log_command;
use crate::vm::VM;

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
    /// Json-encoded file for VM machine configuration
    #[arg(long)]
    machine_spec: JsonFile<MachineOpts>,
    /// Json-encoded file describing paths of binaries required by VM
    #[arg(long)]
    runtime_spec: JsonFile<RuntimeOpts>,
    /// Json-encoded file controlling execution of the VM
    #[arg(long)]
    args_spec: JsonFile<VMArgs>,
}

#[derive(Debug, Args)]
struct IsolateArgs {
    /// Path to container image. VM will be spawned inside the container.
    #[arg(long)]
    image: PathBuf,
    /// List of env variable names to pass through into the container.
    #[arg(long)]
    envs: Option<Vec<String>>,
    /// Args for run command
    #[command(flatten)]
    vm_args: RunArgs,
}

fn respawn(args: &IsolateArgs) -> Result<()> {
    let isolated = isolated(&args.image, args.envs.as_deref())?;
    let exe = env::current_exe().context("while getting argv[0]")?;
    log_command(
        isolated
            .into_command()
            .arg(&exe)
            .arg("run")
            .arg("--machine-spec")
            .arg(args.vm_args.machine_spec.path())
            .arg("--runtime-spec")
            .arg(args.vm_args.runtime_spec.path())
            .arg("--args-spec")
            .arg(args.vm_args.args_spec.path()),
    )
    .status()?;
    Ok(())
}

fn run(args: &RunArgs) -> Result<()> {
    if !is_isolated()? {
        return Err(anyhow!("run must be called from inside container"));
    }
    debug!("RuntimeOpts: {:?}", args.runtime_spec);
    debug!("MachineOpts: {:?}", args.machine_spec);
    debug!("ArgsOpts: {:?}", args.args_spec);

    set_runtime(args.runtime_spec.clone().into_inner())
        .map_err(|_| anyhow!("Failed to set runtime"))?;
    Ok(VM::new(
        args.machine_spec.clone().into_inner(),
        args.args_spec.clone().into_inner(),
    )?
    .run()?)
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
