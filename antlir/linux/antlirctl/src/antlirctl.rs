/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use std::collections::VecDeque;
use std::path::PathBuf;
use structopt::clap::AppSettings;
use structopt::StructOpt;

mod mount;

#[derive(StructOpt)]
#[structopt(name = "antlirctl", setting(AppSettings::NoBinaryName))]
enum AntlirCtl {
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
    match options {
        AntlirCtl::Mount(opts) => mount::mount(opts),
    }
}
