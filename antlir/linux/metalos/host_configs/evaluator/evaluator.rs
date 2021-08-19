/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use structopt::StructOpt;

use evalctx::{Generator, Host};

#[derive(StructOpt)]
struct Opts {
    host: PathBuf,
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
    for gen_path in opts.generators {
        let mut generator_src = String::new();
        File::open(&gen_path)
            .with_context(|| format!("failed to open generator file {}", gen_path.display()))?
            .read_to_string(&mut generator_src)?;
        let gen = Generator::compile(format!("{}", gen_path.display()), generator_src)?;
        let output = gen.eval(&host)?;
        println!("{}\n{:#?}", gen_path.display(), output);
    }
    Ok(())
}

trait PathExt {
    /// Joining absolute paths is annoying, so add an extension trait for
    /// `force_join` which makes it easy.
    fn force_join<P: AsRef<Path>>(&self, path: P) -> PathBuf;
}

impl PathExt for PathBuf {
    fn force_join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        self.as_path().force_join(path)
    }
}

impl PathExt for Path {
    fn force_join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        match path.as_ref().is_absolute() {
            false => self.join(path),
            true => self.join(
                path.as_ref()
                    .strip_prefix("/")
                    .expect("absolute paths will always start with /"),
            ),
        }
    }
}
