/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
use std::fs::File;
use std::io::{self};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use structopt::StructOpt;

use evalctx::{Generator, Host};

#[derive(StructOpt)]
struct Opts {
    host: PathBuf,
    #[structopt(long)]
    root: Option<PathBuf>,
    #[structopt(long)]
    dry_run: bool,
    /// Starlark generator files or directories to use instead of those
    /// installed in '/usr/lib/metalos/generators'
    #[structopt(default_value = "/usr/lib/metalos/generators")]
    generators: Vec<PathBuf>,
}

fn main() -> Result<()> {
    let opts = Opts::from_args();
    let host: Host = {
        if opts.host == Path::new("-") {
            serde_json::from_reader(io::stdin())?
        } else {
            let f = File::open(&opts.host).with_context(|| {
                format!("failed to open host json file {}", opts.host.display())
            })?;
            serde_json::from_reader(f)?
        }
    };
    let generators: Vec<_> = opts
        .generators
        .into_iter()
        .map(|path| Generator::load(&path))
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();
    let mut dry_run = opts.dry_run;
    if !dry_run && opts.root.is_none() {
        eprintln!("--root is missing, assuming --dry-run");
        dry_run = true;
    }
    if dry_run {
        for gen in generators {
            let output = gen.eval(&host)?;
            println!("{}\n{:#?}", gen.name, output);
        }
        return Ok(());
    }
    let root = opts
        .root
        .expect("not running in --dry-run mode, --root must be given");
    for gen in generators {
        let output = gen.eval(&host)?;
        output.apply(&root)?;
    }
    Ok(())
}
