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
mod share;
mod ssh;
mod tpm;
mod types;
mod utils;
mod vm;

use std::collections::HashSet;
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
use maplit::hashset;
use tempfile::tempdir;
use tracing::debug;
use tracing_subscriber::filter::LevelFilter;
use tracing_subscriber::prelude::*;

use crate::isolation::isolated;
use crate::isolation::Platform;
use crate::share::NinePShare;
use crate::share::VirtiofsShare;
use crate::types::MachineOpts;
use crate::types::VMArgs;
use crate::utils::create_tpx_logs;
use crate::utils::env_names_to_kvpairs;
use crate::utils::log_command;
use crate::vm::VMError;
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
    /// Expects the VM to timeout or terminate early
    #[arg(long)]
    expect_failure: bool,
    /// The command should be run after VM termination. Console log will be
    /// available at env $CONSOLE_OUTPUT.
    #[clap(long)]
    postmortem: bool,
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
    /// Extra RW bind-mount into the VM for debugging purpose
    #[arg(long)]
    scratch_dir: Option<PathBuf>,
    /// Args for run command
    #[clap(flatten)]
    run_cmd_args: RunCmdArgs,
}

/// Actually starting the VM. This needs to be inside an ephemeral container as
/// lots of resources relies on container for clean up.
fn run(args: &RunCmdArgs) -> Result<()> {
    debug!("MachineOpts: {:?}", args.machine_spec);
    debug!("VMArgs: {:?}", args.vm_args);

    let mut vm_args = args.vm_args.clone();
    if args.postmortem {
        if args.vm_args.console_output_file.is_none() {
            bail!("Console output file must be specified to run command after VM termination.");
        }
        if args.vm_args.mode.command.is_none() {
            bail!("Expected to run command after VM termination but no command specified.");
        }
        // Don't run the test command inside the VM. Hijack it with our stub so we shut it down as
        // soon as default target is reached.
        vm_args.mode.command = Some(vec!["exit".into()]);
    }

    let machine_opts = args.machine_spec.clone().into_inner();
    let result = if machine_opts.use_legacy_share {
        VM::<NinePShare>::new(machine_opts, vm_args)?.run()
    } else {
        VM::<VirtiofsShare>::new(machine_opts, vm_args)?.run()
    };

    if !args.expect_failure {
        result?;
    } else {
        match result {
            Ok(_) => bail!("Expected VM to fail but succeeded."),
            Err(e) => match e {
                // Only a subset of errors are allowed
                VMError::BootError { .. }
                | VMError::EarlyTerminationError(_)
                | VMError::SSHCommandResultError(_)
                | VMError::RunError(_) => debug!("VM failed with expected error: {:?}", e),
                _ => bail!("VM failed with unexpected error: {:?}", e),
            },
        }
    }

    if args.postmortem {
        let cmd_args = args
            .vm_args
            .mode
            .command
            .as_ref()
            .expect("Command not specified");
        let mut cmd = Command::new(&cmd_args[0]);
        args.vm_args.command_envs.iter().for_each(|pair| {
            cmd.env(&pair.key, &pair.value);
        });
        cmd.env(
            "CONSOLE_OUTPUT",
            args.vm_args
                .console_output_file
                .as_ref()
                .expect("No console output file"),
        );
        cmd_args.iter().skip(1).for_each(|arg| {
            cmd.arg(arg);
        });
        let status = cmd
            .status()
            .context(format!("Command {:?} failed", cmd_args))?;
        if !status.success() {
            bail!("Command {:?} failed: {:?}", cmd_args, status);
        }
    }

    Ok(())
}

/// Enter isolated container and then respawn itself inside it with `run`
/// command and its parameters.
fn respawn(args: &IsolateCmdArgs) -> Result<()> {
    let mut vm_args = args.run_cmd_args.vm_args.clone();
    let envs = env_names_to_kvpairs(args.passenv.clone());
    vm_args.command_envs = envs.clone();
    if let Some(scratch_dir) = args.scratch_dir.as_ref() {
        vm_args.output_dirs.push(scratch_dir.clone());
    }

    // Let's always capture console output unless it's console mode
    let _console_dir;
    if !vm_args.mode.console && vm_args.console_output_file.is_none() {
        let dir = tempdir().context("Failed to create temp dir for console output")?;
        vm_args.console_output_file = Some(dir.path().join("console.txt"));
        _console_dir = dir;
    }

    antlir2_rootless::unshare_new_userns()?;
    antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;

    let isolated = isolated(
        &args.image,
        envs,
        vm_args
            .get_container_output_dirs()
            .into_iter()
            .chain(writable_devices())
            .collect(),
    )?;
    let exe = env::current_exe().context("while getting argv[0]")?;
    let mut command = isolated.command(exe)?;
    command
        .arg("run")
        .arg("--machine-spec")
        .arg(args.run_cmd_args.machine_spec.path())
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

fn writable_outputs(validated_args: &ValidatedVMArgs) -> HashSet<PathBuf> {
    let mut outputs = validated_args.inner.get_container_output_dirs();
    outputs.extend(writable_devices());
    outputs
}

fn writable_devices() -> HashSet<PathBuf> {
    hashset! {
        // Carry over virtualization support
        "/dev/kvm".into(),
        // And tap networking devices
        "/dev/net/tun".into(),
        // And other device nodes needed by qemu
        "/dev/urandom".into(),
        // RW bind-mount /dev/fuse for running XAR.
        // More details in antlir/antlir2/testing/image_test/src/main.rs.
        "/dev/fuse".into(),
    }
}

/// For some tests, an explicit "list test" step is run against the test binary
/// to discover the tests to run. This command is not our intended test to
/// execute, so it's unnecessarily wasteful to execute it inside the VM. We
/// directly run it inside the container without booting VM.
fn list_test_command(args: &IsolateCmdArgs, validated_args: &ValidatedVMArgs) -> Result<Command> {
    let isolated = isolated(
        &args.image,
        validated_args.inner.command_envs.clone(),
        writable_outputs(validated_args),
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
        writable_outputs(validated_args),
    )?;
    let exe = env::current_exe().context("while getting argv[0]")?;
    let mut command = isolated.command(exe)?;
    command
        .arg("run")
        .arg("--machine-spec")
        .arg(args.run_cmd_args.machine_spec.path());
    if args.run_cmd_args.expect_failure {
        command.arg("--expect-failure");
    }
    if args.run_cmd_args.postmortem {
        command.arg("--postmortem");
    }
    command.args(validated_args.inner.to_args());
    Ok(command)
}

/// `test` is similar to `respawn`, except that we assume control for some
/// inputs instead of allowing caller to pass them in. Some inputs are parsed
/// from the test command.
fn test(args: &IsolateCmdArgs) -> Result<()> {
    let validated_args = get_test_vm_args(&args.run_cmd_args.vm_args, args.passenv.clone())?;
    antlir2_rootless::unshare_new_userns()?;
    antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;
    let mut command = if validated_args.is_list {
        list_test_command(args, &validated_args)
    } else {
        vm_test_command(args, &validated_args)
    }?;
    let status = log_command(&mut command).status()?;
    if !status.success() {
        #[cfg(facebook)]
        bail!(
            "VM run failed: {:?}. Check {} for tips of debugging VM specific test failures.",
            status,
            "https://www.internalfb.com/intern/staticdocs/antlir2/docs/internals/fb/vm-tests/",
        );
        #[cfg(not(facebook))]
        bail!("VM run failed: {:?}", status);
    }
    Ok(())
}

fn main() -> Result<()> {
    tracing_subscriber::registry()
        .with(tracing_subscriber::fmt::Layer::default().with_writer(std::io::stderr))
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
