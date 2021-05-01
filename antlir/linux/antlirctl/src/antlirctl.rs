/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
#![feature(str_split_once)]

use std::collections::VecDeque;
use std::path::PathBuf;

use anyhow::Result;
use slog::o;
use structopt::clap::AppSettings;
use structopt::StructOpt;

mod generator;
mod kernel_cmdline;
mod mount;
mod systemd;

#[derive(StructOpt)]
#[structopt(name = "antlirctl", setting(AppSettings::NoBinaryName))]
enum AntlirCtl {
    /// Systemd unit generator
    AntlirGenerator(generator::Opts),
    /// Simplistic method to mount filesystems
    Mount(mount::Opts),
}

fn main() -> Result<()> {
    let mut args: VecDeque<_> = std::env::args_os().collect();
    // Yeah, expect() is not the best thing to do, but really what else can we
    // do besides panic?
    let bin_path: PathBuf = args
        .pop_front()
        .expect("antlirctl: must have argv[0]")
        .into();
    let bin_name = bin_path
        .file_name()
        .expect("antlirctl: argv[0] must be a file path");
    // If argv[0] is a symlink for a multicall utility, push the file name back
    // into the args array so that structopt will parse it correctly
    if bin_name != "antlirctl" {
        args.push_front(bin_name.to_owned());
    }

    let options = AntlirCtl::from_iter(args);

    let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());

    match options {
        AntlirCtl::AntlirGenerator(opts) => generator::generator(log, opts),
        AntlirCtl::Mount(opts) => mount::mount(opts),
    }
}
