/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

use std::collections::BTreeSet;
use std::collections::HashMap;
use std::fs;
use std::fs::File;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use goblin::elf::Elf;
use once_cell::sync::Lazy;
use serde_json::json;
use slog::debug;
use slog::o;
use slog::warn;
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

    /// Iterator of all parent paths. Different from components() in that each
    /// item is a full path
    fn parents(&self) -> Vec<&Path>;
}

impl PathExt for PathBuf {
    fn force_join<P: AsRef<Path>>(&self, path: P) -> PathBuf {
        self.as_path().force_join(path)
    }

    fn parents(&self) -> Vec<&Path> {
        self.as_path().parents()
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

    fn parents(&self) -> Vec<&Path> {
        let mut parents = vec![];
        let mut last = self;
        while let Some(p) = last.parent() {
            parents.push(p);
            last = p;
        }
        parents
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

    /// Full target name of the source target layer to extract from
    #[structopt(long)]
    target: String,

    /// Directory to put the end result json file
    #[structopt(long)]
    output_dir: String,
}

fn main() -> Result<()> {
    let opt = ExtractOpts::from_args();
    let top_src_dir = opt.src_dir;
    let top_dst_dir = opt.dst_dir;
    let target = opt.target;
    let output_dir = opt.output_dir;

    let binaries: Vec<Binary> = opt
        .binaries
        .into_iter()
        .map(|s| Binary::new(top_src_dir.clone(), top_src_dir.force_join(&s), s.into()))
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
            let src = fs::canonicalize(ex.from)
                .unwrap()
                .strip_prefix(top_src_dir.clone())
                .unwrap()
                .to_path_buf();
            (dst, src)
        })
        .collect();

    let copied_dirs: BTreeSet<_> = copy_files
        .keys()
        .flat_map(|k| k.parents())
        // if not under top_dst_dir, then ignore it (ex: it is the parent of top_dst_dir)
        .filter_map(|k| k.strip_prefix(&top_dst_dir).ok().map(|p| p.to_path_buf()))
        .map(|k| (k.components().count(), k))
        .collect();

    let mut features = vec![];
    // "clone" feature for each files to copy
    for (dst, src) in &copy_files {
        features.push(json!(
            {
                "remove_paths": [
                    {
                        "must_exist": false,
                        "path": dst
                    }
                ],
                "target": target
            }
        ));
        features.push(json!(
            {
                "clone": [
                    {
                        "dest": dst,
                        "omit_outer_dir": false,
                        "pre_existing_dest": false,
                        "source": {
                            "content_hash": null,
                            "layer": {
                                    "__BUCK_LAYER_TARGET": target
                            },
                            "path": src,
                            "source": null
                        },
                        "source_layer": {
                        "__BUCK_LAYER_TARGET": target
                        }
                    }
                ],
                "target": target
            }
        ));
    }

    // "ensure_subdirs_exist" feature for each dirs to copy
    for (_, dst_rel) in copied_dirs.iter().rev() {
        if dst_rel.as_os_str().is_empty() {
            continue;
        }
        let dst_dir = top_dst_dir.force_join(&dst_rel);
        let maybe_src_dir = top_src_dir.force_join(dst_rel);
        let mut mode = json!(null);
        // do a bottom-up traversal of all the destination directories, copying the
        // permission bits from the source where possible
        if maybe_src_dir.exists() {
            let meta = fs::metadata(&maybe_src_dir).with_context(|| {
                format!("failed to get permissions from src {:?}", &maybe_src_dir)
            })?;
            mode = json!(meta.permissions().mode() & 0o7777);
        } else {
            warn!(
                LOGGER,
                "leaving default mode for {:?}, because {:?} did not exist", dst_dir, maybe_src_dir
            );
        }
        features.push(json!({
            "ensure_subdirs_exist": [
                {
                    "into_dir": dst_dir.parent().unwrap(),
                    "mode": mode,
                    "subdirs_to_create": dst_dir.file_name().unwrap().to_str().unwrap(),
                    "user": "root",
                    "group": "root"
                }
            ],
            "target": target
        }));
    }

    serde_json::to_writer(
        &File::create(Path::new(&output_dir).force_join("feature.json"))?,
        &json!({
            "features": features,
            "target": target
        }),
    )?;
    Ok(())
}
