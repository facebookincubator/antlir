/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(iter_intersperse)]

mod gen;
mod ir;
mod parse;

use std::convert::TryInto;
use std::fs::File;
use std::path::PathBuf;

use anyhow::{Context, Result};
use structopt::{clap::arg_enum, StructOpt};

use gen::{render, Bzl, Pyo3};
use ir::AllTypes;
use parse::ParsedTop;

#[derive(StructOpt)]
struct Opts {
    input: PathBuf,
    target: RenderTarget,
    output: PathBuf,
}

arg_enum! {
    #[derive(Debug)]
    enum RenderTarget {
        Bzl,
        Pyo3,
    }
}

fn main() -> Result<()> {
    let opts = Opts::from_args();
    let f = File::open(&opts.input)
        .with_context(|| format!("failed to open {}", opts.input.display()))?;
    let input = ParsedTop::from_reader(f)?;
    let types: AllTypes = input
        .try_into()
        .context("Failed to convert from parsed format to internal format")?;
    let code = match opts.target {
        RenderTarget::Bzl => render::<Bzl>(&types),
        RenderTarget::Pyo3 => render::<Pyo3>(&types),
    }
    .context("Trying to render output code")?;
    std::fs::write(&opts.output, code)
        .with_context(|| format!("failed to write to {}", opts.output.display()))?;
    Ok(())
}
