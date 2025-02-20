/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::fs::File;
use std::fs::Permissions;
use std::io::BufWriter;
use std::io::Write;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::Child;
use std::process::Command;
use std::process::Stdio;
use std::time::Duration;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use tracing::error;
use tracing::trace;
use wait_timeout::ChildExt;

#[derive(Parser, Debug)]
struct Args {
    // crosvm is not end-user configurable, but the kernel, rootfs, timeout, etc
    // are, so later occurrences are allowed to override the defaults
    #[clap(long)]
    crosvm: PathBuf,
    #[clap(long, overrides_with = "kernel")]
    kernel: PathBuf,
    #[clap(long, overrides_with = "rootfs")]
    rootfs: PathBuf,
    #[clap(long, overrides_with = "timeout_ms")]
    timeout_ms: Option<u64>,
    /// Command to be run inside the vm inside the rootfs
    #[clap(last(true))]
    cmd: Vec<OsString>,
}

struct SharedDir {
    path: PathBuf,
    tag: String,
    cache: &'static str,
}

impl SharedDir {
    fn to_arg(&self) -> OsString {
        let mut arg: OsString = self.path.clone().into();
        arg.push(":");
        arg.push(&self.tag);
        arg.push(":type=fs");
        arg.push(":cache=");
        arg.push(self.cache);
        arg
    }
}

fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();
    antlir2_rootless::unshare_new_userns().context("while entering userns")?;

    let control_dir = tempfile::TempDir::new()?;

    let mut script = BufWriter::new(File::create(control_dir.path().join("script"))?);
    writeln!(script, "#!/bin/bash\nset -e")?;
    writeln!(script, "cd /__antlir2_appliance_vm__/cwd")?;
    if let Ok(scratch) = std::env::var("BUCK_SCRATCH_PATH") {
        writeln!(script, "export BUCK_SCRATCH_PATH={scratch}")?;
    }
    if let Ok(out) = std::env::var("OUT") {
        writeln!(script, "export OUT={out}")?;
    }
    for arg in args.cmd {
        script.write_all(arg.as_bytes())?;
        script.write_all(b" ")?;
    }
    script
        .into_inner()
        .context("while flushing script")?
        .set_permissions(Permissions::from_mode(0o555))
        .context("while making script executable")?;

    let mut dirs = vec![
        SharedDir {
            path: control_dir.path().to_owned(),
            tag: "control".into(),
            cache: "never",
        },
        SharedDir {
            path: args.rootfs.canonicalize()?,
            tag: "rootfs".into(),
            cache: "always",
        },
        SharedDir {
            path: std::env::current_dir()?,
            tag: "cwd".into(),
            cache: "auto",
        },
    ];
    #[cfg(facebook)]
    dirs.extend([
        SharedDir {
            path: "/mnt/gvfs".into(),
            tag: "gvfs".into(),
            cache: "always",
        },
        SharedDir {
            path: "/usr/local/fbcode".into(),
            tag: "fbcode_runtime".into(),
            cache: "always",
        },
    ]);
    let mut cmd = Command::new(args.crosvm);
    cmd.arg("run")
        .arg(args.kernel)
        .arg("--mem=1024")
        .arg("--serial")
        .arg("type=stdout,hardware=virtio-console")
        .args(
            dirs.into_iter()
                .flat_map(|dir| vec!["--shared-dir".into(), dir.to_arg()]),
        )
        .args([
            "--params",
            "rootfstype=virtiofs root=rootfs init=/__antlir2_appliance_vm__/init",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .stdin(Stdio::null());
    trace!("invoking crosvm: {cmd:?}");
    let mut child = cmd.spawn().context("while spawning crosvm")?;
    let result = if let Some(timeout_ms) = args.timeout_ms {
        child
            .wait_timeout(Duration::from_millis(timeout_ms))
            .context("while waiting for crosvm")?
    } else {
        Some(child.wait().context("while waiting for crosvm")?)
    };
    let status = match result {
        Some(status) => status,
        None => {
            error!("crosvm did not exit within timeout, killing it");
            child.kill().context("while killing crosvm")?;
            child.wait().context("while waiting for killed crosvm")?;
            copy_child_outputs(&mut child)?;
            bail!("vm timed out");
        }
    };

    trace!("crosvm exited: {:?}", status);
    if !status.success() {
        eprintln!("crosvm failed: {:?}", status);
        eprintln!("check the logs for why the vm didn't execute as expected");
        copy_child_outputs(&mut child)?;
        std::process::exit(253);
    }
    let exitcode: i32 = match std::fs::read_to_string(control_dir.path().join("exitcode")) {
        Ok(exitcode) => exitcode
            .trim()
            .parse()
            .with_context(|| format!("invalid exitcode '{exitcode}'")),
        Err(e) => {
            copy_child_outputs(&mut child)?;
            Err(e).context(
                "failed to read exitcode from control dir - check logs for why vm failed to start",
            )
        }
    }?;
    let mut stdout =
        File::open(control_dir.path().join("stdout")).context("while opening stdout file")?;
    std::io::copy(&mut stdout, &mut std::io::stdout())?;
    let mut stderr =
        File::open(control_dir.path().join("stderr")).context("while opening stderr file")?;
    std::io::copy(&mut stderr, &mut std::io::stderr())?;
    std::process::exit(exitcode);
}

fn copy_child_outputs(child: &mut Child) -> Result<()> {
    std::io::copy(
        &mut child.stdout.take().expect("piped"),
        &mut std::io::stdout(),
    )?;
    std::io::copy(
        &mut child.stderr.take().expect("piped"),
        &mut std::io::stderr(),
    )?;
    Ok(())
}
