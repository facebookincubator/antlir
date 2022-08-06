/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;

use anyhow::Result;
use clap::Parser;
use sandbox::sandbox;
use sandbox::SandboxOpts;

#[derive(Parser)]
struct Args {
    #[clap(
        long,
        help = "don't applying strict seccomp filters for non-deterministic syscalls"
    )]
    no_seccomp: bool,
    #[clap(long, help = "read-only bind some files into the sandbox 'SRC[:DST]'")]
    bind_ro: Vec<String>,
    #[clap(long)]
    setenv: Vec<String>,
    binary: OsString,
    binary_args: Vec<OsString>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let ro_files: HashMap<_, _> = args
        .bind_ro
        .iter()
        .map(|p| match p.split_once(':') {
            Some((src, dst)) => (src.into(), dst.into()),
            None => (p.into(), p.into()),
        })
        .collect();
    Err(sandbox(
        args.binary,
        SandboxOpts::builder()
            .seccomp(!args.no_seccomp)
            .ro_files(ro_files)
            .build()?,
    )?
    .args(args.binary_args)
    .envs(args.setenv.iter().map(|setenv| {
        setenv
            .split_once('=')
            .expect("--setenv must be 'key=value' pairs")
    }))
    .exec()
    .into())
}
