/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::Reverse;
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Error, Result};
use goblin::elf::{
    dynamic::{DT_RPATH, DT_RUNPATH},
    Elf,
};
use structopt::StructOpt;

/// Joining absolute paths is annoying, so add an extension trait for
/// `force_join` which makes it easy.
trait ForceJoin {
    fn force_join<P: AsRef<Path>>(&self, path: P) -> PathBuf;
}

impl ForceJoin for PathBuf {
    fn force_join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        self.as_path().force_join(path)
    }
}

impl ForceJoin for Path {
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

#[derive(Debug)]
struct ExtractBinary {
    pub src: PathBuf,
    pub dst: PathBuf,
}

impl std::str::FromStr for ExtractBinary {
    type Err = Error;
    fn from_str(s: &str) -> Result<Self> {
        let parts: Vec<_> = s.splitn(2, ':').collect();
        match parts.len() {
            0 | 1 => Err(Error::msg("expected exactly 2 paths")),
            2 => Ok(Self {
                src: parts[0].into(),
                dst: parts[1].into(),
            }),
            _ => unreachable!(),
        }
    }
}

#[derive(Debug)]
enum Dependency {
    // Most dependencies are absolute. Copy a file from an absolute search path
    // (usually /usr/lib64 from the interpreter search path, or an absolute path
    // in a binaries RPATH) to the same location in `dst`.
    Absolute(PathBuf),

    // Dependencies that are discovered via RPATH with $ORIGIN should be copied
    // to the same binary-relative location as they were found. This is commonly
    // used in buck-built repo binaries with sibling `#symlink-tree` directories
    Relative(PathBuf),
}

impl ExtractBinary {
    fn search_paths(&self, elf: &Elf) -> Result<Vec<Dependency>> {
        let mut paths = Vec::new();
        // RPATH/RUNPATH is searched first
        if let Some(ref dynamic) = elf.dynamic {
            paths.extend(
                dynamic
                    .dyns
                    .iter()
                    .filter(|d| d.d_tag == DT_RPATH || d.d_tag == DT_RUNPATH)
                    .filter_map(|d| elf.dynstrtab.get(d.d_val as usize))
                    .filter_map(|s| s.ok())
                    .map(|runpath| {
                        runpath.split(':').map(|p| match p.contains("$ORIGIN") {
                            true => Dependency::Relative(
                                p.replace("$ORIGIN/", "").replace("$ORIGIN", "").into(),
                            ),
                            false => Dependency::Absolute(p.into()),
                        })
                    })
                    .flatten(),
            );
        }
        if let Some(interp) = elf.interpreter {
            let interp_dir = Path::new(interp).parent().unwrap();
            // resolve a symlink in the common case of the interpreter being in
            // /lib64, which is usually a symlink to /usr/lib64
            let interp_dir = fs::read_link(interp_dir).unwrap_or(interp_dir.to_path_buf());
            paths.push(Dependency::Absolute(interp_dir));
        }
        Ok(paths)
    }

    fn dependencies(&self, root: &Path) -> Result<Vec<Dependency>> {
        let bytes = std::fs::read(root.force_join(&self.src))
            .with_context(|| format!("failed to load binary '{:?}'", &self.src))?;
        let elf = Elf::parse(&bytes).context("failed to parse elf")?;
        let search_paths = self.search_paths(&elf)?;
        elf.libraries
            .iter()
            .map(|lib| {
                for search in &search_paths {
                    match search {
                        Dependency::Absolute(path) => {
                            let libpath = root.force_join(path.join(lib));
                            if libpath.exists() {
                                return Ok(Dependency::Absolute(path.join(lib)));
                            }
                        }
                        Dependency::Relative(path) => {
                            let libpath = self.src.parent().unwrap().join(path).join(lib);
                            if libpath.exists() {
                                let relpath = path.join(lib);
                                return Ok(Dependency::Relative(relpath));
                            }
                        }
                    }
                }
                Err(anyhow!("Dependency not found: {}", lib))
            })
            .chain(
                elf.interpreter
                    .map(|interp| {
                        let interp_dir = Path::new(interp).parent().unwrap();
                        // resolve a symlink in the common case of the interpreter being in
                        // /lib64, which is usually a symlink to /usr/lib64
                        let interp_dir =
                            fs::read_link(interp_dir).unwrap_or(interp_dir.to_path_buf());
                        let interp = interp_dir.join(Path::new(interp).file_name().unwrap());
                        vec![Ok(Dependency::Absolute(interp))]
                    })
                    .unwrap_or(vec![]),
            )
            .collect()
    }
}

#[derive(StructOpt, Debug)]
struct ExtractOpts {
    /// Root source directory for where to find binaries
    #[structopt(long)]
    src_dir: PathBuf,

    /// Root destination directory for where to copy binaries
    #[structopt(long)]
    dst_dir: PathBuf,

    /// Binaries to extract. Accepts pairs of 'src:dst' paths, relative to
    /// --src-dir/--dst-dir.
    #[structopt(long = "binary")]
    binaries: Vec<ExtractBinary>,
}

fn main() -> Result<()> {
    let opt = ExtractOpts::from_args();
    let src_dir = opt.src_dir;
    let dst_dir = opt.dst_dir;
    // map dst -> src to dedupe copy operations
    let copy_files: HashMap<_, _> = opt
        .binaries
        .into_iter()
        .map(|binary| {
            let binary_dst = binary.dst.clone();
            let binary_src = binary.src.clone();
            binary
                .dependencies(src_dir.as_path())
                .unwrap()
                .into_iter()
                .map(move |dep| {
                    match dep {
                        Dependency::Relative(dep) => (
                            binary.dst.parent().unwrap().join(&dep),
                            binary.src.parent().unwrap().join(&dep),
                        ),
                        Dependency::Absolute(dep) => (dep.clone(), dep),
                    }
                })
                .chain(vec![(binary_dst, binary_src)])
        })
        .flatten()
        .collect();
    for (dst, src) in &copy_files {
        let dst = dst_dir.force_join(dst);
        let src = src_dir.force_join(src);
        eprintln!("copying {:?} -> {:?}", src, dst);
        fs::create_dir_all(dst.parent().unwrap()).context("failed to create dest dir")?;
        fs::copy(&src, &dst).with_context(|| format!("failed to copy {:?} -> {:?}", src, dst))?;
    }

    // do a bottom-up traversal of all the destination directories, copying the
    // permission bits from the source where possible
    let mut dst_dirs: Vec<_> = copy_files.keys().collect();
    dst_dirs.sort_unstable_by_key(|k| Reverse(k.components().count()));
    for dst in dst_dirs {
        let dst_abs = dst_dir.force_join(dst);
        let src = src_dir.force_join(dst);
        if let Ok(meta) = fs::metadata(src) {
            fs::set_permissions(&dst_abs, meta.permissions())
                .with_context(|| format!("failed to set permissions on {:?}", dst_abs))?;
        }
    }

    Ok(())
}
