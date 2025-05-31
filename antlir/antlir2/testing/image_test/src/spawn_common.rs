/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::collections::HashMap;
use std::fs::File;
use std::fs::Permissions;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::fs::PermissionsExt;
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_isolate::InvocationType;
use antlir2_isolate::IsolationContext;
use antlir2_isolate::nspawn;
use antlir2_isolate::unshare;
use anyhow::Context;
use anyhow::Result;
use anyhow::ensure;
use bon::builder;
use image_test_lib::Test;
use tempfile::NamedTempFile;
use tracing::debug;
use tracing::trace;

use crate::exec;
use crate::runtime;

fn make_log_files(_base: &str) -> Result<(NamedTempFile, NamedTempFile)> {
    Ok((NamedTempFile::new()?, NamedTempFile::new()?))
}

#[builder]
pub(crate) fn run(
    spec: runtime::Spec,
    test: Test,
    #[builder(default)] interactive: bool,
) -> Result<()> {
    let repo = find_root::find_repo_root(std::env::current_exe().context("while getting argv[0]")?)
        .context("while looking for repo root")?
        .canonicalize()
        .context("while canonicalizing repo root")?;

    if spec.rootless {
        antlir2_rootless::unshare_new_userns().context("while unsharing userns")?;
    }

    let mut setenv: BTreeMap<_, _> = spec.setenv.into_iter().collect();
    // forward test runner env vars to the inner test
    for (key, val) in std::env::vars() {
        if key.starts_with("TEST_PILOT") {
            setenv.insert(key, val);
        }
    }
    for key in spec.pass_env {
        let var = std::env::var(&key).with_context(|| format!("--pass-env var '{key}' missing"))?;

        setenv.insert(key, var);
    }
    if let Ok(rust_log) = std::env::var("RUST_LOG") {
        setenv.insert("RUST_LOG".into(), rust_log);
    }

    let working_directory = std::env::current_dir().context("while getting cwd")?;

    let mut ctx = IsolationContext::builder(&spec.layer);
    ctx.platform([
        // test is built out of the repo, so it needs the
        // repo to be available
        repo.as_path(),
        #[cfg(facebook)]
        Path::new("/mnt/gvfs"),
    ]);
    if cfg!(facebook) && spec.mount_platform {
        ctx.platform([Path::new("/usr/local/fbcode")]);
    }
    ctx.inputs([
        // tests often read resource files from the repo
        repo.as_path(),
    ])
    .working_directory(&working_directory)
    .setenv(setenv.clone())
    .outputs(test.output_dirs())
    .invocation_type(match (spec.boot.is_some(), interactive) {
        (true, false) => InvocationType::BootReadOnly,
        (false, false) => InvocationType::Pid2Pipe,
        (true, true) => InvocationType::BootInteractive,
        (false, true) => InvocationType::Pid2Interactive,
    })
    .inputs(spec.mounts)
    .setenv(("ANTLIR2_IMAGE_TEST", "1"));

    // XARs need /dev/fuse to run. Ideally we could just have this created
    // inside the container. Until
    // https://github.com/systemd/systemd/issues/17607 is resolved, we need to
    // rw bind-mount /dev/fuse in
    if Path::new("/dev/fuse").exists() {
        ctx.outputs([Path::new("/dev/fuse")]);
    }
    if spec.rootless {
        #[cfg(facebook)]
        ctx.tmpfs(Path::new("/mnt/xarfuse"));

        // these should be tmpfs, just like systemd-nspawn does
        ctx.tmpfs(Path::new("/tmp")).tmpfs(Path::new("/run"));
    }

    if let Some(hostname) = spec.hostname {
        ctx.hostname(hostname);
    }

    // test output dirs/files need to be world-writable so that tests can run as
    // unprivileged users that are not the build user
    for path in test.output_dirs() {
        std::fs::set_permissions(&path, Permissions::from_mode(0o777))
            .with_context(|| format!("while making {} world-writable", path.display()))?;
    }

    // bind LLVM coverage output paths
    if let Some(llvm_profile_file) = std::env::var_os("LLVM_PROFILE_FILE") {
        // TPX overrides LLVM_PROFILE_FILE when --collect-coverage is set
        if llvm_profile_file != "/dev/null" {
            ctx.outputs(
                Path::new(&llvm_profile_file)
                    .parent()
                    .context("LLVM_PROFILE_FILE did not have parent")?
                    .to_owned(),
            );
        }
    }

    if spec.rootless {
        ctx.devtmpfs(Path::new("/dev"));
    }

    match spec.boot {
        Some(boot) => {
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
                .arg(&spec.layer)
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

            for unit in &boot.requires_units {
                let unit = match unit.as_str() {
                    "default.target" => default_target,
                    unit => unit,
                };
                writeln!(test_unit_dropin, "Requires={unit}")?;
            }
            for unit in boot.requires_units.iter().chain(&boot.after_units) {
                let unit = match unit.as_str() {
                    "default.target" => default_target,
                    unit => unit,
                };
                writeln!(test_unit_dropin, "After={unit}")?;
            }
            for unit in boot.wants_units.iter() {
                let unit = match unit.as_str() {
                    "default.target" => default_target,
                    unit => unit,
                };
                writeln!(test_unit_dropin, "Wants={unit}")?;
            }

            if interactive {
                let shell_dropin = buck_resources::get(
                    "antlir/antlir2/testing/image_test/antlir2_image_test_shell.conf",
                )
                .context("while getting shell dropin resource")?;
                ctx.inputs((
                    PathBuf::from("/run/systemd/system/antlir2_image_test.service.d/99-shell.conf"),
                    shell_dropin,
                ));
            }

            // wire the test output to the parent process's std{out,err}
            ctx.outputs(HashMap::from([
                (Path::new("/antlir2/test_stdout"), test_stdout.path()),
                (Path::new("/antlir2/test_stderr"), test_stderr.path()),
            ]));
            ctx.inputs((
                Path::new("/run/systemd/system/antlir2_image_test.service.d/00-runtime.conf"),
                test_unit_dropin.path(),
            ));

            let mut exec_env = setenv.clone();
            // forward test runner env vars to the inner test
            for (key, val) in std::env::vars() {
                if key.starts_with("TEST_PILOT") {
                    exec_env.insert(key, val);
                }
            }

            let exec_spec = exec::Spec::builder()
                .cmd(test.into_inner_cmd())
                .user(spec.user)
                .working_directory(std::env::current_dir().context("while getting cwd")?)
                .env(exec_env)
                .build();
            let exec_spec_file =
                tempfile::NamedTempFile::new().context("while creating temp file for exec spec")?;
            serde_json::to_writer_pretty(&exec_spec_file, &exec_spec)
                .context("while serializing exec spec to file")?;
            ctx.inputs((
                Path::new("/__antlir2_image_test__/exec_spec.json"),
                exec_spec_file.path(),
            ));

            let mut isol = if spec.rootless {
                let mut isol = unshare(ctx.build())?.command("/usr/lib/systemd/systemd")?;
                isol.arg("systemd.unit=antlir2_image_test.service");
                isol
            } else {
                nspawn(ctx.build())?.command("systemd.unit=antlir2_image_test.service")?
            };

            isol.arg("systemd.journald.forward_to_console=1")
                .arg("systemd.log_time=1")
                .arg("systemd.setenv=ANTLIR2_IMAGE_TEST=1");
            debug!("executing test in booted isolated container: {isol:?}");
            let mut child = isol
                // the stdout/err of the systemd inside the container is a pipe
                // so that we can print it IFF the test fails
                .stdout(container_stdout.as_file()?)
                .stderr(container_stdout.as_file()?)
                .spawn()
                .context("while spawning systemd-nspawn")?;
            let res = child.wait().context("while waiting for systemd-nspawn")?;

            std::io::copy(&mut test_stdout, &mut std::io::stdout())?;
            std::io::copy(&mut test_stderr, &mut std::io::stderr())?;

            if !res.success() {
                // if the container stdout is not already being dumped to
                // stdout/err, then print out the path where it can be found
                if let ContainerConsoleOutput::File { path, .. } = &container_stdout {
                    eprintln!(
                        "full container console output can be found at: '{}'",
                        path.display()
                    );
                    eprintln!("{}", path.display());
                }
                std::process::exit(res.code().unwrap_or(255))
            } else {
                Ok(())
            }
        }
        None => {
            // some systems-y tests want to read /sys
            ctx.inputs(Path::new("/sys"));
            ctx.user(spec.user);
            let mut cmd = test.into_inner_cmd().into_iter();
            let program = cmd.next().expect("must have program arg");
            let mut isol = match spec.rootless {
                false => nspawn(ctx.build())?.command(program)?,
                true => unshare(ctx.build())?.command(program)?,
            };
            isol.args(cmd);
            debug!("executing test in isolated container: {isol:?}");
            Err(anyhow::anyhow!("failed to exec test: {:?}", isol.exec()))
        }
    }
}

enum ContainerConsoleOutput {
    File { path: PathBuf, file: File },
    Stderr,
}

impl ContainerConsoleOutput {
    fn as_file(&self) -> Result<File> {
        match self {
            Self::File { file, .. } => file.try_clone().context("while cloning file fd"),
            Self::Stderr => Ok(unsafe { File::from_raw_fd(std::io::stderr().as_raw_fd()) }),
        }
    }
}

/// Create a file to record container stdout into. When invoked under tpx, this
/// will be uploaded as an artifact. The artifact metadata is set up before
/// running the test so that it still gets uploaded even in case of a timeout
fn container_stdout_file() -> Result<ContainerConsoleOutput> {
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
        File::create(&dst)
            .with_context(|| format!("while creating {}", dst.display()))
            .map(|file| ContainerConsoleOutput::File { path: dst, file })
    } else {
        // otherwise, have it go right to stderr
        Ok(ContainerConsoleOutput::Stderr)
    }
}
