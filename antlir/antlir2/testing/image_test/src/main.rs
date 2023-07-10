/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::BTreeSet;
use std::collections::HashMap;
use std::collections::HashSet;
use std::ffi::OsString;
use std::fs::File;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::str::FromStr;

use antlir2_features::mount::Mount;
use antlir2_isolate::isolate;
use antlir2_isolate::InvocationType;
use antlir2_isolate::IsolationContext;
use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;
use tempfile::NamedTempFile;
use tracing::debug;
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
    /// Boot the container with /init as pid1 before running the test
    boot: bool,
    #[clap(long = "requires-unit", requires = "boot")]
    /// Add Requires= and After= dependencies on these units
    requires_units: Vec<String>,
    #[clap(long)]
    /// Set these env vars in the test environment
    setenv: Vec<KvPair>,
    #[clap(long)]
    /// Mounts required by the layer-under-test
    mounts: JsonFile<BTreeSet<Mount<'static>>>,
    #[clap(subcommand)]
    test: Test,
}

#[derive(Parser, Debug)]
enum Test {
    Custom {
        test_cmd: Vec<OsString>,
    },
    Gtest {
        #[clap(long, env = "GTEST_OUTPUT")]
        output: Option<String>,
        #[clap(allow_hyphen_values = true)]
        test_cmd: Vec<OsString>,
    },
    Pyunit {
        #[clap(long)]
        list_tests: Option<PathBuf>,
        #[clap(long)]
        output: Option<PathBuf>,
        #[clap(long)]
        test_filter: Vec<OsString>,
        test_cmd: Vec<OsString>,
    },
    Rust {
        #[clap(allow_hyphen_values = true)]
        test_cmd: Vec<OsString>,
    },
}

impl Test {
    /// Some tests need to write to output paths on the host. Instead of a
    /// complicated fd-passing dance in the name of isolation purity, we just
    /// mount the parent directories of the output files so that the inner test
    /// can do writes just as tpx expects.
    fn bind_mounts(&self) -> HashSet<PathBuf> {
        match self {
            Self::Custom { .. } => HashSet::new(),
            Self::Gtest { output, .. } => match output {
                Some(output) => {
                    let path = Path::new(match output.split_once(':') {
                        Some((_format, path)) => path,
                        None => output.as_str(),
                    });
                    HashSet::from([path
                        .parent()
                        .expect("output file always has parent")
                        .to_owned()])
                }
                None => HashSet::new(),
            },
            Self::Rust { .. } => HashSet::new(),
            Self::Pyunit {
                list_tests, output, ..
            } => {
                let mut paths = HashSet::new();
                if let Some(p) = list_tests {
                    paths.insert(
                        p.parent()
                            .expect("output file always has parent")
                            .to_owned(),
                    );
                }
                if let Some(p) = output {
                    paths.insert(
                        p.parent()
                            .expect("output file always has parent")
                            .to_owned(),
                    );
                }
                paths
            }
        }
    }
    fn into_inner_cmd(self) -> Vec<OsString> {
        match self {
            Self::Custom { test_cmd } => test_cmd,
            Self::Gtest {
                mut test_cmd,
                output,
            } => {
                if let Some(out) = output {
                    test_cmd.push(format!("--gtest_output={out}").into());
                }
                test_cmd
            }
            Self::Rust { test_cmd } => test_cmd,
            Self::Pyunit {
                mut test_cmd,
                list_tests,
                test_filter,
                output,
            } => {
                if let Some(list) = list_tests {
                    test_cmd.push("--list-tests".into());
                    test_cmd.push(list.into());
                }
                if let Some(out) = output {
                    test_cmd.push("--output".into());
                    test_cmd.push(out.into());
                }
                for filter in test_filter {
                    test_cmd.push("--test-filter".into());
                    test_cmd.push(filter);
                }
                test_cmd
            }
        }
    }
}

#[derive(Debug, Clone)]
struct KvPair(String, OsString);

impl FromStr for KvPair {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.split_once('=') {
            Some((key, value)) => Ok(Self(key.to_owned(), value.trim_matches('"').into())),
            None => Err(anyhow!("expected = separated kv pair, got '{s}'")),
        }
    }
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
        .map(|pair| (pair.0, pair.1))
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
    .outputs(args.test.bind_mounts())
    .invocation_type(match args.boot {
        true => InvocationType::BootReadOnly,
        false => InvocationType::Pid2Pipe,
    });
    ctx.inputs(
        args.mounts
            .into_inner()
            .into_iter()
            .map(|mount| match mount {
                Mount::Host(m) => (m.mountpoint.into_owned(), m.src),
                Mount::Layer(m) => (m.mountpoint.into_owned(), m.src.subvol_symlink.into_owned()),
            })
            .collect::<HashMap<_, _>>(),
    );

    // XARs need /dev/fuse to run. Ideally we could just have this created
    // inside the container. Until
    // https://github.com/systemd/systemd/issues/17607 is resolved, we need to
    // rw bind-mount /dev/fuse in
    if Path::new("/dev/fuse").exists() {
        ctx.outputs([Path::new("/dev/fuse")]);
    }

    if args.boot {
        // Mark the kernel-command-line.service unit as being Type=simple so
        // that the boot graph is considered complete as soon as it starts the
        // test.
        let mut dropin = NamedTempFile::new()?;
        writeln!(dropin, "[Unit]")?;
        // do not exit the container until the test itself is done
        writeln!(dropin, "SuccessAction=none")?;
        // if, however, kernel-command-line.service fails to even start the
        // test, exit immediately
        writeln!(dropin, "FailureAction=exit-force")?;
        writeln!(dropin, "[Service]")?;
        writeln!(dropin, "Type=simple")?;
        // kernel-command-line.service will just start the
        // antlir2_image_test.service unit that is created below. That unit has {Failure,Success}Action
        let systemd_run_arg = "systemd.run=\"systemctl start antlir2_image_test.service\"";
        ctx.inputs((
            Path::new("/run/systemd/system/kernel-command-line.service.d/antlir2.conf"),
            dropin.path(),
        ));

        let container_stdout = container_stdout_file()?;
        let (mut test_stdout, mut test_stderr) = make_log_files("test")?;

        let mut test_unit = NamedTempFile::new()?;
        writeln!(test_unit, "[Unit]")?;
        // exit the container as soon as this test is done, using the exit code
        // of the process
        writeln!(test_unit, "SuccessAction=exit-force")?;
        writeln!(test_unit, "FailureAction=exit-force")?;
        for unit in &args.requires_units {
            writeln!(test_unit, "After={unit}")?;
            writeln!(test_unit, "Requires={unit}")?;
        }

        writeln!(test_unit, "[Service]")?;
        // Having Type=simple will not cause a test that waits for `systemctl
        // is-system-running` to stall until the test itself is done (which
        // would never happen). {Failure,Success}Action are still respected when
        // the test process exits either way.
        writeln!(test_unit, "Type=simple")?;

        write!(test_unit, "WorkingDirectory=")?;
        let cwd = std::env::current_dir().context("while getting cwd")?;
        test_unit.write_all(cwd.as_os_str().as_bytes())?;
        test_unit.write_all(b"\n")?;

        write!(test_unit, "ExecStart=")?;
        let mut iter = args.test.into_inner_cmd().into_iter().peekable();
        if let Some(exe) = iter.next() {
            let realpath = std::fs::canonicalize(&exe)
                .with_context(|| format!("while getting absolute path of {exe:?}"))?;
            test_unit.write_all(realpath.as_os_str().as_bytes())?;
            if iter.peek().is_some() {
                test_unit.write_all(b" ")?;
            }
        }
        while let Some(arg) = iter.next() {
            test_unit.write_all(arg.as_os_str().as_bytes())?;
            if iter.peek().is_some() {
                test_unit.write_all(b" ")?;
            }
        }
        test_unit.write_all(b"\n")?;

        // wire the test output to the parent process's std{out,err}
        write!(test_unit, "StandardOutput=truncate:")?;
        test_unit.write_all(test_stdout.path().as_os_str().as_bytes())?;
        test_unit.write_all(b"\n")?;
        write!(test_unit, "StandardError=truncate:")?;
        test_unit.write_all(test_stderr.path().as_os_str().as_bytes())?;
        test_unit.write_all(b"\n")?;

        writeln!(test_unit, "Environment=USER=%u")?;

        for (key, val) in &setenv {
            write!(test_unit, "Environment=\"{key}=")?;
            test_unit.write_all(val.as_bytes())?;
            writeln!(test_unit, "\"")?;
        }
        // forward test runner env vars to the inner test
        for (key, val) in std::env::vars() {
            if key.starts_with("TEST_PILOT") {
                writeln!(test_unit, "Environment=\"{key}={val}\"")?;
            }
        }
        ctx.outputs(test_stdout.path());
        ctx.outputs(test_stderr.path());
        ctx.inputs((
            Path::new("/run/systemd/system/antlir2_image_test.service"),
            test_unit.path(),
        ));

        let mut isol = isolate(ctx.build()).into_command();
        isol.arg(systemd_run_arg)
            .arg("systemd.journald.forward_to_console=1")
            .arg("systemd.log_time=1");
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
        let mut isol = isolate(ctx.build()).into_command();
        isol.args(args.test.into_inner_cmd());
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
