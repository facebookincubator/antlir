/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bindgen::builder;
use clap::Parser;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    header: String,
    #[clap(long)]
    out: PathBuf,
    /// Arguments for clang, e.g. include paths for the header's includes
    #[clap(last = true)]
    clang_args: Vec<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    clang_sys::load()
        .map_err(Error::msg)
        .context("while loading libclang")?;

    let bindings = builder()
        .header(&args.header)
        .clang_args(&args.clang_args)
        .generate()?;

    bindings.write_to_file(&args.out)?;
    Ok(())
}
