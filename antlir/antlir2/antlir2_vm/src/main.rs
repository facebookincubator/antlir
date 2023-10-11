/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

mod disk;
mod isolation;
mod net;
mod pci;
mod runtime;
mod share;
mod ssh;
mod tpm;
mod types;
mod utils;
mod vm;

use std::env;
use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use anyhow::anyhow;
use anyhow::bail;
use anyhow::Context;
use clap::Args;
use clap::Parser;
use clap::Subcommand;
use image_test_lib::KvPair;
use image_test_lib::Test;
use json_arg::JsonFile;
use tempfile::tempdir;
use tracing::debug;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;

use crate::isolation::is_isolated;
use crate::isolation::isolated;
use crate::isolation::Platform;
use crate::runtime::set_runtime;
use crate::types::MachineOpts;
use crate::types::RuntimeOpts;
use crate::types::VMArgs;
use crate::utils::create_tpx_logs;
use crate::utils::env_names_to_kvpairs;
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
    /// Run the VM. Must be executed inside container.
    Run(RunCmdArgs),
    /// Respawn inside isolated image and execute `Run` command.
    Isolate(IsolateCmdArgs),
    /// Run VM tests inside container.
    Test(IsolateCmdArgs),
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
    /// Pass through these environment variables into the container and VM, if they exist.
    #[arg(long)]
    passenv: Vec<String>,
    /// Args for run command
    #[clap(flatten)]
    run_cmd_args: RunCmdArgs,
}

/// Actually starting the VM. This needs to be inside an ephemeral container as
/// lots of resources relies on container for clean up.
fn run(args: &RunCmdArgs) -> Result<()> {
    if !is_isolated()? {
        return Err(anyhow!("run must be called from inside container"));
    }
    debug!("RuntimeOpts: {:?}", args.runtime_spec);
    debug!("MachineOpts: {:?}", args.machine_spec);
    debug!("VMArgs: {:?}", args.vm_args);

    set_runtime(args.runtime_spec.clone().into_inner())
        .map_err(|_| anyhow!("Failed to set runtime"))?;
    Ok(VM::new(args.machine_spec.clone().into_inner(), args.vm_args.clone())?.run()?)
}

/// Enter isolated container and then respawn itself inside it with `run`
/// command and its parameters.
fn respawn(args: &IsolateCmdArgs) -> Result<()> {
    let mut vm_args = args.run_cmd_args.vm_args.clone();
    let envs = env_names_to_kvpairs(args.passenv.clone());
    vm_args.command_envs = envs.clone();

    // Let's always capture console output unless it's console mode
    let _console_dir;
    if vm_args.console_output_file.is_none() {
        let dir = tempdir().context("Failed to create temp dir for console output")?;
        vm_args.console_output_file = Some(dir.path().join("console.txt"));
        _console_dir = dir;
    }

    let isolated = isolated(&args.image, envs, vm_args.get_container_output_dirs())?;
    let exe = env::current_exe().context("while getting argv[0]")?;
    let mut command = isolated.command(exe)?;
    command
        .arg("run")
        .arg("--machine-spec")
        .arg(args.run_cmd_args.machine_spec.path())
        .arg("--runtime-spec")
        .arg(args.run_cmd_args.runtime_spec.path())
        .args(vm_args.to_args());

    let status = log_command(&mut command).status()?;
    if !status.success() {
        bail!("VM run failed: {:?}", status);
    }
    Ok(())
}

/// Validated `VMArgs` and other necessary metadata for tests.
struct ValidatedVMArgs {
    /// VMArgs that will be passed into the VM with modified fields
    inner: VMArgs,
    /// True if the test command is listing tests
    is_list: bool,
}

/// Record and upload envs for debugging purpose
#[cfg(not(test))]
fn record_envs(envs: &[KvPair]) -> Result<()> {
    let env_file = create_tpx_logs("env", "env vars")?;
    if let Some(file) = env_file {
        std::fs::write(
            file,
            envs.iter()
                .map(|s| s.to_os_string().to_string_lossy().to_string())
                .collect::<Vec<_>>()
                .join("\n"),
        )?;
    }
    Ok(())
}

/// We only want envs for actual VM test, not unit tests here.
#[cfg(test)]
fn record_envs(_envs: &[KvPair]) -> Result<()> {
    Ok(())
}

/// Further validate `VMArgs` parsed by clap and generate a new `VMArgs` with
/// content specific to test execution.
fn get_test_vm_args(orig_args: &VMArgs, cli_envs: Vec<String>) -> Result<ValidatedVMArgs> {
    if orig_args.timeout_secs.is_none() {
        return Err(anyhow!("Test command must specify --timeout-secs."));
    }
    if !orig_args.output_dirs.is_empty() {
        return Err(anyhow!(
            "Test command must not specify --output-dirs. \
            This will be parsed from env and test command parameters instead."
        ));
    }

    // Forward test runner env vars to the inner test
    let mut env_names = cli_envs;
    for (key, _) in std::env::vars() {
        if key.starts_with("TEST_PILOT") {
            env_names.push(key);
        }
    }
    let envs = env_names_to_kvpairs(env_names);
    record_envs(&envs)?;

    #[derive(Debug, Parser)]
    struct TestArgsParser {
        #[clap(subcommand)]
        test: Test,
    }
    let mut orig_command = vec![OsString::from("bogus_exec")];
    orig_command.extend_from_slice(
        &orig_args
            .mode
            .command
            .clone()
            .ok_or(anyhow!("Test command must not be empty"))?,
    );
    let test_args = TestArgsParser::try_parse_from(orig_command)
        .context("Test command does not match expected format of `<type> <command>`")?;
    let is_list = test_args.test.is_list_tests();
    let mut vm_args = orig_args.clone();
    vm_args.output_dirs = test_args.test.output_dirs().into_iter().collect();
    vm_args.mode.command = Some(test_args.test.into_inner_cmd());
    vm_args.command_envs = envs;
    vm_args.console_output_file = create_tpx_logs("console", "console logs")?;
    Ok(ValidatedVMArgs {
        inner: vm_args,
        is_list,
    })
}

/// For some tests, an explicit "list test" step is run against the test binary
/// to discover the tests to run. This command is not our intended test to
/// execute, so it's unnecessarily wasteful to execute it inside the VM. We
/// directly run it inside the container without booting VM.
fn list_test_command(args: &IsolateCmdArgs, validated_args: &ValidatedVMArgs) -> Result<Command> {
    let mut output_dirs = validated_args.inner.get_container_output_dirs();
    // RW bind-mount /dev/fuse for running XAR.
    // More details in antlir/antlir2/testing/image_test/src/main.rs.
    output_dirs.insert(PathBuf::from("/dev/fuse"));
    let isolated = isolated(
        &args.image,
        validated_args.inner.command_envs.clone(),
        output_dirs,
    )?;
    let mut inner_cmd = validated_args
        .inner
        .mode
        .command
        .as_ref()
        .expect("command must exist here")
        .iter();
    let mut command = isolated.command(inner_cmd.next().expect("must have program arg"))?;
    command.args(inner_cmd);
    Ok(command)
}

/// For actual test command, we spawn the VM and run it.
fn vm_test_command(args: &IsolateCmdArgs, validated_args: &ValidatedVMArgs) -> Result<Command> {
    let isolated = isolated(
        &args.image,
        validated_args.inner.command_envs.clone(),
        validated_args.inner.get_container_output_dirs(),
    )?;
    let exe = env::current_exe().context("while getting argv[0]")?;
    let mut command = isolated.command(exe)?;
    command
        .arg("run")
        .arg("--machine-spec")
        .arg(args.run_cmd_args.machine_spec.path())
        .arg("--runtime-spec")
        .arg(args.run_cmd_args.runtime_spec.path())
        .args(validated_args.inner.to_args());
    Ok(command)
}

/// `test` is similar to `respawn`, except that we assume control for some
/// inputs instead of allowing caller to pass them in. Some inputs are parsed
/// from the test command.
fn test(args: &IsolateCmdArgs) -> Result<()> {
    let validated_args = get_test_vm_args(&args.run_cmd_args.vm_args, args.passenv.clone())?;
    let mut command = if validated_args.is_list {
        list_test_command(args, &validated_args)
    } else {
        vm_test_command(args, &validated_args)
    }?;
    let status = log_command(&mut command).status()?;
    if !status.success() {
        bail!("VM run failed: {:?}", status);
    }
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::Layer::default())
        .with(
            tracing_subscriber::EnvFilter::builder()
                .with_default_directive(LevelFilter::INFO.into())
                .from_env()
                .expect("Invalid logging level set by env"),
        )
        .init();
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
    use crate::types::VMModeArgs;

    #[test]
    fn test_get_test_vm_args() {
        let valid = VMArgs {
            timeout_secs: Some(1),
            mode: VMModeArgs {
                command: Some(["custom", "whatever"].iter().map(OsString::from).collect()),
                ..Default::default()
            },
            ..Default::default()
        };
        let mut expected = valid.clone();
        expected.mode.command = Some(vec![OsString::from("whatever")]);
        let parsed = get_test_vm_args(&valid, vec![]).expect("Parsing should succeed");
        assert_eq!(parsed.inner.mode, expected.mode);
        assert!(!parsed.is_list);

        let mut timeout = valid.clone();
        timeout.timeout_secs = None;
        assert!(get_test_vm_args(&timeout, vec![]).is_err());

        let mut output_dirs = valid.clone();
        output_dirs.output_dirs = vec![PathBuf::from("/some")];
        assert!(get_test_vm_args(&output_dirs, vec![]).is_err());

        let mut command = valid.clone();
        command.mode.command = None;
        assert!(get_test_vm_args(&command, vec![]).is_err());
        command.mode.command = Some(vec![OsString::from("invalid")]);
        assert!(get_test_vm_args(&command, vec![]).is_err());

        let env_var_test = valid;
        std::env::set_var("TEST_PILOT_A", "A");
        let parsed = get_test_vm_args(&env_var_test, vec![]).expect("Parsing should succeed");
        assert!(
            parsed
                .inner
                .command_envs
                .iter()
                .any(|x| *x == KvPair::from(("TEST_PILOT_A", "A")))
        );
    }
}
