/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{bail, Context, Result};
use goblin::elf::Elf;
use once_cell::sync::Lazy;
use slog::{debug, o, warn};
use structopt::StructOpt;

static LOGGER: Lazy<slog::Logger> = Lazy::new(|| slog_glog_fmt::facebook_logger().unwrap());
static LDSO_RE: Lazy<regex::Regex> = Lazy::new(|| {
    regex::RegexBuilder::new(r#"^\s*(?P<name>.+)\s+=>\s+(?P<path>.+)\s+\(0x[0-9a-f]+\)$"#)
        .multi_line(true)
        .build()
        .unwrap()
});

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

#[derive(Debug, Clone)]
struct ExtractFile {
    /// Absolute path (including --src-dir if applicable) where a file should be
    /// copied from.
    from: PathBuf,
    /// Absolute path (not including --dst-dir) where a file should be copied to.
    to: PathBuf,
}

#[derive(Debug)]
struct Binary {
    // root to consider for this binary, either / or --src-dir
    root: PathBuf,
    file: ExtractFile,
    interpreter: PathBuf,
}

/// In all the cases that we care about, a library will live under /lib64, but
/// this directory will be a symlink to /usr/lib64. To avoid build conflicts with
/// other image layers, replace it.
fn ensure_usr<P: AsRef<Path>>(path: P) -> PathBuf {
    match path.as_ref().starts_with("/usr") {
        true => path.as_ref().to_path_buf(),
        false => Path::new("/usr/").force_join(path),
    }
}

impl Binary {
    pub fn new(root: PathBuf, src: PathBuf, dst: PathBuf) -> Result<Self> {
        let bytes =
            std::fs::read(&src).with_context(|| format!("failed to load binary '{:?}'", &src))?;

        let elf = Elf::parse(&bytes).context("failed to parse elf")?;
        let interpreter: PathBuf = elf
            .interpreter
            .unwrap_or_else(|| {
                // The PT_INTERP header in an ELF allows setting an explicit
                // interpreter. However, this may be omitted (as is the case in
                // some of the libraries we explicitly extract), in which case
                // we can just use a sensible default.
                warn!(LOGGER, "no interpreter found for {:?}, using default '/usr/lib64/ld-linux-x86-64.so.2'", &src);
                "/usr/lib64/ld-linux-x86-64.so.2"
            })
            .into();

        Ok(Self {
            root,
            file: ExtractFile { from: src, to: dst },
            interpreter,
        })
    }

    pub fn try_parse_buck(s: &str) -> Result<Self> {
        let parts: Vec<_> = s.splitn(2, ':').collect();
        if parts.len() != 2 {
            bail!("expected two colon-separated paths");
        }
        Self::new("/".into(), parts[0].into(), parts[1].into())
    }

    /// Find all transitive dependencies of this binary. Return ExtractFile
    /// structs for this binary, its interpreter and all dependencies.
    fn extracts(&self) -> Result<Vec<ExtractFile>> {
        let log = LOGGER.new(o!("binary" => self.file.from.to_string_lossy().to_string()));

        let output = Command::new(self.root.force_join(&self.interpreter))
            .arg("--list")
            .arg(&self.file.from)
            .output()
            .with_context(|| format!("failed to list libraries for {:?}", self.file.from))?;
        let ld_output_str =
            std::str::from_utf8(&output.stdout).context("ld.so output not utf-8")?;

        Ok(LDSO_RE
            .captures_iter(ld_output_str)
            .map(|cap| {
                let name = cap.name("name").unwrap().as_str();
                let path = Path::new(cap.name("path").unwrap().as_str());
                debug!(log, "{} provided by {:?}", name, path);
                // There is not a bulletproof way to tell if a dependency is
                // supposed to be relative to the source location or not based
                // solely on the ld.so output.
                // As a simple heuristic, guess that if the directory is the
                // same as that of the binary, it should be installed at the
                // same relative location to the binary destination.
                // Importantly, this heuristic can only ever produce an
                // incorrect result with buck-built binaries (the only kind
                // where destination is not necessarily the same as the source),
                // and if a binary really has an absolute dependency on buck-out,
                // there is nothing we can do about it.
                let bin_src_parent = self.file.from.parent().unwrap();
                if path.starts_with(&bin_src_parent) {
                    debug!(log, "{} seems to be installed at a relative path", name);
                    let relpath = path.strip_prefix(bin_src_parent).unwrap();
                    let bin_dst_parent = self.file.to.parent().unwrap();
                    return ExtractFile {
                        from: self.root.force_join(path),
                        to: bin_dst_parent.join(relpath),
                    };
                }
                ExtractFile {
                    from: path.to_path_buf(),
                    to: ensure_usr(path),
                }
            })
            // also include the binary itself and its interpreter
            .chain(vec![
                self.file.clone(),
                ExtractFile {
                    from: self.root.force_join(&self.interpreter),
                    to: ensure_usr(&self.interpreter),
                },
            ])
            .collect())
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

    /// Binaries to extract. Extracts the given absolute paths from --src-dir
    /// into the same location in --dst-dir.
    #[structopt(long = "binary")]
    binaries: Vec<String>,

    /// Buck-built binaries to extract. Same format as --binary, but src is
    /// treated as an absolute path.
    #[structopt(long = "buck-binary")]
    buck_binaries: Vec<String>,
}

fn main() -> Result<()> {
    let opt = ExtractOpts::from_args();
    let top_src_dir = opt.src_dir;
    let top_dst_dir = opt.dst_dir;

    let binaries: Vec<Binary> = opt
        .binaries
        .into_iter()
        .map(|s| Binary::new(top_src_dir.clone(), top_src_dir.force_join(&s), s.into()))
        .chain(
            opt.buck_binaries
                .into_iter()
                .map(|s| Binary::try_parse_buck(&s)),
        )
        .collect::<Result<_>>()?;

    let extract_files: Vec<_> = binaries
        .into_iter()
        .map(|bin| bin.extracts())
        .collect::<Result<Vec<_>>>()?
        .into_iter()
        .flatten()
        .collect();

    // map dst -> src to dedupe file copies for libraries that might be depended
    // on multiple times
    let copy_files: HashMap<PathBuf, PathBuf> = extract_files
        .into_iter()
        .map(|ex| {
            let dst = top_dst_dir.force_join(ex.to);
            (dst, ex.from)
        })
        .collect();

    for (dst, src) in &copy_files {
        debug!(LOGGER, "copying {:?} -> {:?}", src, dst);
        fs::create_dir_all(dst.parent().unwrap()).context("failed to create dest dir")?;
        fs::copy(src, dst)?;
    }

    Ok(())
}
