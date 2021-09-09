/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
use std::ffi::OsStr;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use structopt::StructOpt;

use evalctx::{generator::GeneratorOutput, Generator, Host};

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

/// Recursively load a directory of .star generators or individual file paths
fn load_generators(path: &Path) -> Result<Vec<Generator>> {
    match std::fs::metadata(path)?.is_dir() {
        true => Ok(std::fs::read_dir(path)
            .with_context(|| format!("failed to list generators in {}", path.display()))?
            .into_iter()
            .filter_map(Result::ok)
            .filter(|entry| entry.file_type().map(|t| !t.is_symlink()).unwrap_or(false))
            .map(|entry| load_generators(&entry.path()))
            .collect::<Result<Vec<_>>>()?
            .into_iter()
            .flatten()
            .collect()),
        false => match path.extension() == Some(OsStr::new("star")) {
            true => compile_generator(path).map(|gen| vec![gen]),
            false => Ok(vec![]),
        },
    }
}

fn compile_generator(path: &Path) -> Result<Generator> {
    let src = std::fs::read_to_string(path)
        .with_context(|| format!("failed to open generator file {}", path.display()))?;
    Generator::compile(format!("{}", path.display()), src)
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
        .map(|path| load_generators(&path))
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
        apply_generator(&root, output)?;
    }
    Ok(())
}

fn apply_generator(root: &Path, output: GeneratorOutput) -> Result<()> {
    for file in output.files {
        let dst = root.force_join(file.path);
        let mut f =
            File::create(&dst).with_context(|| format!("failed to create {}", dst.display()))?;
        f.write_all(&file.contents)
            .with_context(|| format!("failed to write {}", dst.display()))?;
        let mut perms = f.metadata()?.permissions();
        perms.set_mode(file.mode);
        f.set_permissions(perms)
            .with_context(|| format!("failed to set mode of {}", dst.display()))?;
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
