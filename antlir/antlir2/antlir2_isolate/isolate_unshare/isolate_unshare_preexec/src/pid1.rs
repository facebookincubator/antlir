/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use isolate_cfg::IsolationContext;
use json_arg::Json;
use nix::sys::wait::waitpid;
use nix::sys::wait::WaitStatus;
use nix::unistd::Pid;
use tokio::process::Command;
use tokio::runtime::Runtime;
use tokio::signal::unix::signal;
use tokio::signal::unix::SignalKind;

use crate::isolation;

#[derive(Parser, Debug)]
pub(crate) struct Pid1Args {
    isolation: Json<IsolationContext<'static>>,
    program: OsString,
    #[clap(last = true)]
    program_args: Vec<OsString>,
}

pub(crate) fn handler(args: Pid1Args) -> Result<()> {
    Runtime::new()
        .context("while creating tokio runtime")?
        .block_on(pid1_async(args))
}

async fn pid1_async(args: Pid1Args) -> Result<()> {
    ensure!(std::process::id() == 1, "pid1 stub must be run as pid1");

    // setup signal handlers before doing anything else so that this can be
    // killed as necessary
    let mut sigchld = signal(SignalKind::child())?;
    let mut sigterm = signal(SignalKind::terminate())?;

    isolation::setup_isolation(args.isolation.as_inner())?;

    let mut pid2 = Command::new(args.program);
    pid2.env_clear();
    // reasonable default PATH (same as systemd-nspawn uses)
    pid2.env(
        "PATH",
        "/usr/local/sbin:/usr/local/bin:/usr/sbin:/usr/bin:/sbin:/bin",
    );
    pid2.env("container", "antlir2");
    pid2.env("USER", args.isolation.user.as_ref());
    if let Some(term) = std::env::var_os("TERM") {
        pid2.env("TERM", term);
    }
    for (key, val) in &args.isolation.setenv {
        pid2.env(key, val);
    }
    pid2.args(args.program_args);
    let mut pid2 = pid2.spawn().context("while spawning pid2")?;
    // I call this pid2, but it might not actually be 2, so grab it now
    let pid2_id = Pid::from_raw(pid2.id().context("while getting pid2 pid")? as i32);
    let pid2_wait = pid2.wait();
    tokio::pin!(pid2_wait);
    loop {
        tokio::select! {
            pid2_status = &mut pid2_wait => {
                // If our main "tracked" pid2 exits, just exit with the same
                // status code. The kernel will clean up any processes that
                // might be left over.
                let pid2_status = pid2_status.context("while waiting for pid2")?;
                std::process::exit(pid2_status.code().unwrap_or(254));
            }
            _ = sigchld.recv() => {
                // If a process gets reparented to init (this process), then
                // this signal will be received. Loop to clear process wait
                // status until there are none left.
                loop {
                    match waitpid(Pid::from_raw(-1), None) {
                        Ok(WaitStatus::Exited(pid, code)) => {
                            if pid == pid2_id {
                                std::process::exit(code)
                            }
                        }
                        Ok(WaitStatus::Signaled(pid, sig, _)) => {
                            if pid == pid2_id {
                                std::process::exit(128 + sig as i32)
                            }
                        }
                        Err(nix::Error::ECHILD) => {
                            // No more zombie processes
                            break;
                        }
                        _ => continue,
                    }
                }
            }
            _ = sigterm.recv() => {
                // If we get SIGTERM, then just exit and the kernel will
                // forcibly kill any processes left over
                std::process::exit(0);
            }
        }
    }
}
