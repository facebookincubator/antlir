/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

use antlir2_isolate::IsolationContext;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use tracing::trace;

#[derive(Parser, Debug)]
struct Args {
    #[clap(long)]
    build_appliance: PathBuf,
    #[clap(long)]
    input: PathBuf,
    #[clap(long)]
    output: PathBuf,
    #[clap(long)]
    bwrap: PathBuf,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt::init();

    let args = Args::parse();

    let mut cmd = antlir2_isolate::sys::bwrap(
        IsolationContext::builder(args.build_appliance)
            .ephemeral(false)
            .readonly()
            .build(),
        Some(args.bwrap.as_os_str()),
    )?
    .command("/usr/bin/rpm2extents");
    cmd.arg("SHA256")
        .stdin(
            File::open(&args.input)
                .with_context(|| format!("while opening input {}", args.input.display()))?,
        )
        .stdout(
            File::create(&args.output)
                .with_context(|| format!("while opening output {}", args.output.display()))?,
        );

    trace!("isolated command: {cmd:#?}");

    Err(cmd.exec().into())
}
