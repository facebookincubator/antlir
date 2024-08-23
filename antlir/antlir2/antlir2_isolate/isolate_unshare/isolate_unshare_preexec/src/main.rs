/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(io_error_more)]

use std::ffi::OsString;
use std::process::Command;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use isolate_cfg::IsolationContext;
use json_arg::Json;
use nix::sched::unshare;
use nix::sched::CloneFlags;

mod isolation;
mod pid1;
use pid1::Pid1Args;

#[derive(Debug, Parser)]
enum Subcommand {
    Main(Main),
    Pid1(Pid1Args),
}

#[derive(Debug, Parser)]
struct Main {
    isolation: Json<IsolationContext<'static>>,
    program: OsString,
    #[clap(last = true)]
    program_args: Vec<OsString>,
}

fn main() {
    let args = Subcommand::parse();
    if let Err(e) = match args {
        Subcommand::Main(args) => do_main(args),
        Subcommand::Pid1(args) => pid1::handler(args),
    } {
        let e = format!("{e:#?}");
        eprintln!("{e}");
        std::process::exit(1);
    }
}

fn do_main(args: Main) -> Result<()> {
    // Unshare into new pid namespace first, then the rest of the isolation is
    // performed by the first forked process (pid 1) in that namespace
    unshare(CloneFlags::CLONE_NEWPID).context("while unsharing into new pid namespace")?;
    let mut pid1 = Command::new(std::env::current_exe().context("while getting current exe")?);
    pid1.arg("pid1")
        .arg(
            serde_json::to_string(args.isolation.as_inner())
                .context("while serializing isolation info")?,
        )
        .arg(args.program)
        .arg("--")
        .args(args.program_args);
    let mut pid1 = pid1.spawn().context("while spawning pid1")?;
    let status = pid1.wait().context("while waiting for pid1")?;
    if status.success() {
        Ok(())
    } else if let Some(code) = status.code() {
        std::process::exit(code);
    } else {
        Err(anyhow!("pid1 failed: {status}"))
    }
}
