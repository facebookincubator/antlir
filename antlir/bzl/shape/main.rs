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

use anyhow::{Context, Result};
use std::convert::TryInto;

use gen::render;
use ir::AllTypes;
use parse::ParsedTop;

fn main() -> Result<()> {
    let input = ParsedTop::from_reader(std::io::stdin().lock())?;
    eprintln!("{:#?}", input);
    let types: AllTypes = input
        .try_into()
        .context("Failed to convert from parsed format to internal format")?;
    eprintln!("{:#?}", types);
    let code = render(&types).context("Trying to render output code")?;
    println!("{}", code);
    Ok(())
}
