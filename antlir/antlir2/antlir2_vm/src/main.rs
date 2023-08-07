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
use std::ffi::OsString;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use image_test_lib::Test;
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
    /// Respawn inside isolated image and execute `Run` command.
    Isolate(IsolateCmdArgs),
    /// Run the VM. Must be executed inside container.
    Run(RunCmdArgs),
    /// Run VM tests inside container.
    Test(TestCmdArgs),
}

/// Execute the VM
#[derive(Debug, Args)]
struct RunCmdArgs {
    /// Json-encoded file for VM machine configuration
    #[arg(long)]
    machine_spec: JsonFile<MachineOpts>,
    /// Json-encoded file describing paths of binaries required by VM
    #[arg(long)]
    runtime_spec: JsonFile<RuntimeOpts>,
    #[clap(flatten)]
    vm_args: VMArgs,
}

/// Spawn a container and execute the VM inside.
#[derive(Debug, Args)]
struct IsolateCmdArgs {
    /// Path to container image.
    #[arg(long)]
    image: PathBuf,
    /// Args for run command
    #[clap(flatten)]
    run_cmd_args: RunCmdArgs,
}

/// Similar to `isolate` with additional restrictions enforced after parsing.
#[derive(Debug, Args)]
struct TestCmdArgs {
    #[clap(flatten)]
    isolate_cmd_args: IsolateCmdArgs,
}

/// Actually starting the VM. This needs to be inside an ephemeral container as
/// lots of resources relies on container for clean up.
fn run(args: &RunCmdArgs) -> Result<()> {
    if !is_isolated()? {
        return Err(anyhow!("run must be called from inside container"));
    }
    debug!("RuntimeOpts: {:?}", args.runtime_spec);
    debug!("MachineOpts: {:?}", args.machine_spec);

    set_runtime(args.runtime_spec.clone().into_inner())
        .map_err(|_| anyhow!("Failed to set runtime"))?;
    Ok(VM::new(args.machine_spec.clone().into_inner(), args.vm_args.clone())?.run()?)
}

/// Enter isolated container and then respawn itself inside it with `run`
/// command and its parameters.
fn respawn(args: &IsolateCmdArgs) -> Result<()> {
    let isolated = isolated(
        &args.image,
        None, // TODO
        &args.run_cmd_args.vm_args.output_dirs,
    )?;
    let exe = env::current_exe().context("while getting argv[0]")?;
    let mut command = isolated.into_command();
    command
        .arg(&exe)
        .arg("run")
        .arg("--machine-spec")
        .arg(args.run_cmd_args.machine_spec.path())
        .arg("--runtime-spec")
        .arg(args.run_cmd_args.runtime_spec.path());
    command.args(args.run_cmd_args.vm_args.to_args());

    log_command(&mut command).status()?;
    Ok(())
}

/// Further validate `VMArgs` parsed by clap and generate a new `VMArgs` with
/// content specific to test execution.
fn get_test_vm_args(orig_args: &VMArgs) -> Result<VMArgs> {
    if orig_args.timeout_s.is_none() {
        return Err(anyhow!("Test command must specify --timeout-s."));
    }
    if !orig_args.output_dirs.is_empty() {
        return Err(anyhow!(
            "Test command must not specify --output-dirs. \
            This will be parsed from env and test command parameters instead."
        ));
    }

    #[derive(Debug, Parser)]
    struct TestArgsParser {
        #[clap(subcommand)]
        test: Test,
    }
    let mut orig_command = vec![OsString::from("bogus_exec")];
    orig_command.extend_from_slice(
        &orig_args
            .command
            .clone()
            .ok_or(anyhow!("Test command must not be empty"))?,
    );
    let test_args = TestArgsParser::try_parse_from(orig_command)
        .context("Test command does not match expected format of `<type> <command>`")?;
    let mut vm_args = orig_args.clone();
    vm_args.output_dirs = test_args.test.output_dirs().into_iter().collect();
    vm_args.command = Some(test_args.test.into_inner_cmd());
    Ok(vm_args)
}

/// This function is similar to `respawn`, except that we assume control for
/// some inputs instead of allowing caller to pass them in. Some inputs are
/// parsed from test command.
fn test(args: &TestCmdArgs) -> Result<()> {
    let vm_args = get_test_vm_args(&args.isolate_cmd_args.run_cmd_args.vm_args)?;
    let isolated = isolated(
        &args.isolate_cmd_args.image,
        None, // TODO
        &vm_args.output_dirs.iter().collect::<Vec<_>>(),
    )?;

    let exe = env::current_exe().context("while getting argv[0]")?;
    let mut command = isolated.into_command();
    command
        .arg(&exe)
        .arg("run")
        .arg("--machine-spec")
        .arg(args.isolate_cmd_args.run_cmd_args.machine_spec.path())
        .arg("--runtime-spec")
        .arg(args.isolate_cmd_args.run_cmd_args.runtime_spec.path());
    command.args(vm_args.to_args());
    log_command(&mut command).status()?;
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
        Commands::Test(args) => test(args),
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_get_test_vm_args() {
        let valid = VMArgs {
            timeout_s: Some(1),
            console: false,
            output_dirs: vec![],
            command_envs: None,
            command: Some(["custom", "whatever"].iter().map(OsString::from).collect()),
        };
        let mut expected = valid.clone();
        expected.command = Some(vec![OsString::from("whatever")]);
        assert_eq!(
            get_test_vm_args(&valid).expect("Parsing should succeed"),
            expected,
        );

        let mut timeout = valid.clone();
        timeout.timeout_s = None;
        assert!(get_test_vm_args(&timeout).is_err());

        let mut output_dirs = valid.clone();
        output_dirs.output_dirs = vec![PathBuf::from("/some")];
        assert!(get_test_vm_args(&output_dirs).is_err());

        let mut command = valid;
        command.command = None;
        assert!(get_test_vm_args(&command).is_err());
        command.command = Some(vec![OsString::from("invalid")]);
        assert!(get_test_vm_args(&command).is_err());
    }
}
