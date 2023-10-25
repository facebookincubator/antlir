/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs::File;
use std::fs::Permissions;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_isolate::isolate;
use antlir2_isolate::InvocationType;
use antlir2_isolate::IsolationContext;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use image_test_lib::KvPair;
use image_test_lib::Test;
use json_arg::JsonFile;
use loopdev::LoopControl;
use mount::Mount;
use tempfile::NamedTempFile;
use tracing::debug;
use tracing::trace;
use tracing_subscriber::prelude::*;

fn make_log_files(_base: &str) -> Result<(NamedTempFile, NamedTempFile)> {
    Ok((NamedTempFile::new()?, NamedTempFile::new()?))
}

#[derive(Parser, Debug)]
/// Run a unit test inside an image layer.
struct Args {
    #[clap(long)]
    /// Path to layer to run the test in
    layer: PathBuf,
    #[clap(long, default_value = "root")]
    /// Run the test as this user
    user: String,
    #[clap(long)]
    /// Set container hostname
    hostname: Option<String>,
    #[clap(long)]
    /// Boot the container with /init as pid1 before running the test
    boot: bool,
    #[clap(long = "requires-unit", requires = "boot")]
    /// Add Requires= and After= dependencies on these units
    requires_units: Vec<String>,
    #[clap(long = "after-unit", requires = "boot")]
    /// Add an After= dependency on these units
    after_units: Vec<String>,
    #[clap(long = "wants-unit", requires = "boot")]
    /// Add Wants= dependencies on these units
    wants_units: Vec<String>,
    #[clap(long)]
    /// Set these env vars in the test environment
    setenv: Vec<KvPair>,
    #[clap(long)]
    /// Mounts required by the layer-under-test
    mounts: JsonFile<BTreeSet<Mount>>,
    #[clap(long)]
    /// Allocate N loopback devices and bind them into the container
    allocate_loop_devices: u8,
    #[clap(subcommand)]
    test: Test,
}

fn main() -> Result<()> {
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

    let repo = find_root::find_repo_root(
        &absolute_path::AbsolutePathBuf::canonicalize(
            std::env::current_exe().context("while getting argv[0]")?,
        )
        .context("argv[0] not absolute")?,
    )
    .context("while looking for repo root")?;

    let mut setenv: BTreeMap<_, _> = args
        .setenv
        .into_iter()
        .map(|pair| (pair.key, pair.value))
        .collect();
    // forward test runner env vars to the inner test
    for (key, val) in std::env::vars() {
        if key.starts_with("TEST_PILOT") {
            setenv.insert(key, val.into());
        }
    }
    if let Some(rust_log) = std::env::var_os("RUST_LOG") {
        setenv.insert("RUST_LOG".into(), rust_log);
    }

    let working_directory = std::env::current_dir().context("while getting cwd")?;

    let mut ctx = IsolationContext::builder(&args.layer);
    ctx.platform([
        // test is built out of the repo, so it needs the
        // repo to be available
        repo.as_ref(),
        #[cfg(facebook)]
        Path::new("/usr/local/fbcode"),
        #[cfg(facebook)]
        Path::new("/mnt/gvfs"),
    ])
    .inputs([
        // tests often read resource files from the repo
        repo.as_ref(),
    ])
    .working_directory(&working_directory)
    .setenv(setenv.clone())
    .outputs(args.test.output_dirs())
    .invocation_type(match args.boot {
        true => InvocationType::BootReadOnly,
        false => InvocationType::Pid2Pipe,
    });
    ctx.inputs(
        args.mounts
            .into_inner()
            .into_iter()
            .map(|mount| match mount {
                Mount::Host(m) => (m.mountpoint, m.src),
                Mount::Layer(m) => (m.mountpoint, m.src.subvol_symlink),
            })
            .collect::<HashMap<_, _>>(),
    );
    ctx.setenv(("ANTLIR2_IMAGE_TEST", "1"));

    // XARs need /dev/fuse to run. Ideally we could just have this created
    // inside the container. Until
    // https://github.com/systemd/systemd/issues/17607 is resolved, we need to
    // rw bind-mount /dev/fuse in
    if Path::new("/dev/fuse").exists() {
        ctx.outputs([Path::new("/dev/fuse")]);
    }

    if let Some(hostname) = args.hostname {
        ctx.hostname(hostname);
    }

    // test output dirs/files need to be world-writable so that tests can run as
    // unprivileged users that are not the build user
    for path in args.test.output_dirs() {
        std::fs::set_permissions(&path, Permissions::from_mode(0o777))
            .with_context(|| format!("while making {} world-writable", path.display()))?;
    }

    // hang on to open fds of loop devices so they don't get closed
    let mut loop_devices = Vec::new();

    if args.allocate_loop_devices > 0 {
        let lc = LoopControl::open().context("while opening loop control")?;
        for i in 0..args.allocate_loop_devices {
            let ld = lc.next_free().context("while allocating loop device")?;
            let path = std::fs::read_link(format!("/proc/self/fd/{}", ld.as_raw_fd()))
                .context("while getting path of loopdev")?;
            ctx.setenv((format!("ANTLIR2_LOOPDEV_{i}"), path.clone()));
            ctx.outputs(path);
            loop_devices.push(ld);
        }
    }

    if args.boot {
        let container_stdout = container_stdout_file()?;
        let (mut test_stdout, mut test_stderr) = make_log_files("test")?;

        let mut test_unit_dropin = NamedTempFile::new()?;
        writeln!(test_unit_dropin, "[Unit]")?;

        // If a test requires default.target, it really wants the _real_
        // default.target, not the test itself which becomes default.target when
        // we pass systemd.unit=
        let res = Command::new("systemctl")
            .arg("get-default")
            .arg("--root")
            .arg(&args.layer)
            .output()
            .context("while running systemctl get-default")?;
        ensure!(
            res.status.success(),
            "systemctl get-default failed: {}",
            String::from_utf8_lossy(&res.stderr)
        );
        let default_target = std::str::from_utf8(&res.stdout)
            .context("systemctl get-default returned invalid utf8")?
            .trim();
        trace!("default target was {default_target}");

        for unit in &args.requires_units {
            let unit = match unit.as_str() {
                "default.target" => default_target,
                unit => unit,
            };
            writeln!(test_unit_dropin, "Requires={unit}")?;
        }
        for unit in args.requires_units.iter().chain(&args.after_units) {
            let unit = match unit.as_str() {
                "default.target" => default_target,
                unit => unit,
            };
            writeln!(test_unit_dropin, "After={unit}")?;
        }
        for unit in args.wants_units.iter() {
            let unit = match unit.as_str() {
                "default.target" => default_target,
                unit => unit,
            };
            writeln!(test_unit_dropin, "Wants={unit}")?;
        }

        writeln!(test_unit_dropin, "[Service]")?;

        writeln!(test_unit_dropin, "User={}", args.user)?;
        write!(test_unit_dropin, "WorkingDirectory=")?;
        let cwd = std::env::current_dir().context("while getting cwd")?;
        test_unit_dropin.write_all(cwd.as_os_str().as_bytes())?;
        test_unit_dropin.write_all(b"\n")?;

        write!(test_unit_dropin, "Environment=PWD=")?;
        test_unit_dropin.write_all(cwd.as_os_str().as_bytes())?;
        test_unit_dropin.write_all(b"\n")?;

        write!(test_unit_dropin, "ExecStart=")?;
        let mut iter = args.test.into_inner_cmd().into_iter().peekable();
        if let Some(exe) = iter.next() {
            let realpath = std::fs::canonicalize(&exe)
                .with_context(|| format!("while getting absolute path of {exe:?}"))?;
            test_unit_dropin.write_all(realpath.as_os_str().as_bytes())?;
            if iter.peek().is_some() {
                test_unit_dropin.write_all(b" ")?;
            }
        }
        while let Some(arg) = iter.next() {
            test_unit_dropin.write_all(arg.as_os_str().as_bytes())?;
            if iter.peek().is_some() {
                test_unit_dropin.write_all(b" ")?;
            }
        }
        test_unit_dropin.write_all(b"\n")?;

        for (key, val) in &setenv {
            write!(test_unit_dropin, "Environment=\"{key}=")?;
            test_unit_dropin.write_all(val.as_bytes())?;
            writeln!(test_unit_dropin, "\"")?;
        }
        // forward test runner env vars to the inner test
        for (key, val) in std::env::vars() {
            if key.starts_with("TEST_PILOT") {
                writeln!(test_unit_dropin, "Environment=\"{key}={val}\"")?;
            }
        }
        // wire the test output to the parent process's std{out,err}
        ctx.outputs(HashMap::from([
            (Path::new("/antlir2/test_stdout"), test_stdout.path()),
            (Path::new("/antlir2/test_stderr"), test_stderr.path()),
        ]));
        ctx.inputs((
            Path::new("/run/systemd/system/antlir2_image_test.service.d/runtime.conf"),
            test_unit_dropin.path(),
        ));

        // Register the test container with systemd-machined so manual debugging
        // is a easier.
        ctx.register(true);

        let mut isol = isolate(ctx.build())?.command("systemd.unit=antlir2_image_test.service")?;
        isol.arg("systemd.journald.forward_to_console=1")
            .arg("systemd.log_time=1")
            .arg("systemd.setenv=ANTLIR2_IMAGE_TEST=1");
        debug!("executing test in booted isolated container: {isol:?}");
        let mut child = isol
            // the stdout/err of the systemd inside the container is a pipe
            // so that we can print it IFF the test fails
            .stdout(container_stdout.try_clone()?)
            .stderr(container_stdout.try_clone()?)
            .spawn()
            .context("while spawning systemd-nspawn")?;
        let res = child.wait().context("while waiting for systemd-nspawn")?;

        std::io::copy(&mut test_stdout, &mut std::io::stdout())?;
        std::io::copy(&mut test_stderr, &mut std::io::stderr())?;

        if !res.success() {
            std::process::exit(res.code().unwrap_or(255))
        } else {
            Ok(())
        }
    } else {
        ctx.user(args.user);
        let mut cmd = args.test.into_inner_cmd().into_iter();
        let mut isol = isolate(ctx.build())?.command(cmd.next().expect("must have program arg"))?;
        isol.args(cmd);
        debug!("executing test in isolated container: {isol:?}");
        Err(anyhow::anyhow!("failed to exec test: {:?}", isol.exec()))
    }
}

/// Create a file to record container stdout into. When invoked under tpx, this
/// will be uploaded as an artifact. The artifact metadata is set up before
/// running the test so that it still gets uploaded even in case of a timeout
fn container_stdout_file() -> Result<File> {
    // if tpx has provided this artifacts dir, put the logs there so they get
    // uploaded along with the test results
    if let Some(artifacts_dir) = std::env::var_os("TEST_RESULT_ARTIFACTS_DIR") {
        std::fs::create_dir_all(&artifacts_dir)?;
        let dst = Path::new(&artifacts_dir).join("container-stdout.txt");
        if let Some(annotations_dir) = std::env::var_os("TEST_RESULT_ARTIFACT_ANNOTATIONS_DIR") {
            std::fs::create_dir_all(&annotations_dir)?;
            std::fs::write(
                Path::new(&annotations_dir).join("container-stdout.txt.annotation"),
                r#"{"type": {"generic_text_log": {}}, "description": "systemd logs"}"#,
            )?;
        }
        File::create(&dst).with_context(|| format!("while creating {}", dst.display()))
    } else {
        // otherwise, have it go right to stderr
        Ok(unsafe { File::from_raw_fd(std::io::stderr().as_raw_fd()) })
    }
}
