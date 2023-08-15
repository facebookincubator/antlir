/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashSet;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;

use antlir2_isolate::isolate;
use antlir2_isolate::IsolationContext;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;

#[derive(Debug, Parser)]
struct Args {
    /// Path to mounted layer
    layer: PathBuf,
    #[clap(long = "input")]
    inputs: Vec<PathBuf>,
    #[clap(long = "output")]
    outputs: Vec<PathBuf>,
    #[clap(long = "create-output-file")]
    create_output_files: Vec<PathBuf>,
    program: OsString,
    args: Vec<OsString>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    for path in &args.create_output_files {
        std::fs::File::create(path)
            .with_context(|| format!("while creating '{}'", path.display()))?;
    }
    Err(isolate(
        IsolationContext::builder(args.layer)
            .inputs(args.inputs.into_iter().collect::<HashSet<_>>())
            .outputs(args.outputs.into_iter().collect::<HashSet<_>>())
            .outputs(args.create_output_files.into_iter().collect::<HashSet<_>>())
            .working_directory(std::env::current_dir().context("while getting cwd")?)
            .build(),
    )?
    .command(args.program)?
    .args(args.args)
    .exec()
    .into())
}
