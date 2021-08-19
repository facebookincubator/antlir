/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
use std::ffi::OsStr;
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

fn extract_generators(archive_path: &Path) -> Result<Vec<Generator>> {
    let f = File::open(archive_path)
        .with_context(|| format!("failed to open tarball {}", archive_path.display()))?;
    let dec = zstd::Decoder::new(f)?;
    let mut ar = tar::Archive::new(dec);
    let mut generators = vec![];
    for file in ar.entries().context("failed to get entries from tar")? {
        let mut f = file.context("invalid file found in tar")?;
        let path = f.path().context("invalid filename")?.into_owned();
        if path.extension() == Some(OsStr::new("star")) {
            let mut src = String::new();
            f.read_to_string(&mut src)?;
            generators.push(Generator::compile(
                format!("{}:{}", archive_path.display(), path.display()),
                src,
            )?);
        }
    }
    Ok(generators)
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
    let mut generators = vec![];
    for gen_path in opts.generators {
        if gen_path.extension() == Some(OsStr::new("star")) {
            let mut src = String::new();
            File::open(&gen_path)
                .with_context(|| format!("failed to open generator file {}", gen_path.display()))?
                .read_to_string(&mut src)?;
            generators.push(Generator::compile(format!("{}", gen_path.display()), src)?);
        } else if gen_path.extension() == Some(OsStr::new("zst")) {
            generators.extend(extract_generators(&gen_path)?);
        } else {
            eprintln!(
                "Ignoring generator '{}' with unknown extension",
                gen_path.display()
            );
        }
    }
    for gen in generators {
        let output = gen.eval(&host)?;
        println!("{}\n{:#?}", gen.name, output);
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
