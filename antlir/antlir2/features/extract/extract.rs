/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::BTreeSet;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::File;
use std::hash::Hasher;
use std::io::BufRead;
use std::io::BufReader;
use std::path::Path;
use std::path::PathBuf;

use antlir2_compile::Arch;
use antlir2_compile::CompilerContext;
use antlir2_compile::util::copy_with_metadata;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::Validator;
use antlir2_depgraph_if::item::FileType;
use antlir2_depgraph_if::item::FsEntry;
use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::item::ItemKey;
use antlir2_depgraph_if::item::Path as PathItem;
use antlir2_features::types::PathInLayer;
use antlir2_path::PathExt;
use anyhow::Context;
use anyhow::Result;
use goblin::elf::Elf;
use serde::Deserialize;
use serde::Serialize;
use serde::de::Deserializer;
use serde::de::Error as _;
use tracing::trace;
use tracing::warn;
use twox_hash::XxHash64;

pub type Feature = Extract;

/// An entry in the extract manifest describing a file to install.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub enum ManifestEntry {
    /// Copy a file from libs_dir to an absolute path in the image
    File { src_relpath: PathBuf, dst: PathBuf },
    /// Create a symlink at `link` pointing to `target`
    Symlink { link: PathBuf, target: PathBuf },
}

/// A manifest of files to install in the image, produced by an analysis action
/// and consumed by the extract feature compile step.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Manifest(pub BTreeSet<ManifestEntry>);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Extract {
    pub provides: Vec<PathInLayer>,
    pub libs: Libs,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Libs {
    #[serde(deserialize_with = "Libs::deserialize_manifest_file")]
    manifest: Manifest,
    libs_dir: PathBuf,
}

impl Libs {
    fn deserialize_manifest_file<'de, D>(deserializer: D) -> Result<Manifest, D::Error>
    where
        D: Deserializer<'de>,
    {
        let path = PathBuf::deserialize(deserializer)?;
        let f =
            BufReader::new(File::open(&path).map_err(|e| {
                D::Error::custom(format!("failed to open {}: {e}", path.display()))
            })?);
        serde_json::from_reader(f).map_err(D::Error::custom)
    }
}

impl antlir2_depgraph_if::RequiresProvides for Extract {
    fn provides(&self) -> Result<Vec<Item>, String> {
        // Intentionally provide only the direct files the user asked for,
        // because we don't want to produce conflicts with all the transitive
        // dependencies. However, we will check that any duplicated items are in
        // fact identical, to prevent insane mismatches like this
        // https://fb.workplace.com/groups/btrmeup/posts/5913570682055882
        Ok(self
            .provides
            .iter()
            .map(|path| {
                Item::Path(PathItem::Entry(FsEntry {
                    path: path.to_owned(),
                    file_type: FileType::File,
                    mode: 0o555,
                }))
            })
            .collect())
    }

    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(self
            .provides
            .iter()
            .map(|path| {
                Requirement::ordered(
                    ItemKey::Path(path.parent().expect("dst always has parent").to_owned()),
                    Validator::FileType(FileType::Directory),
                )
            })
            .collect())
    }
}

impl antlir2_compile::CompileFeature for Extract {
    #[tracing::instrument(name = "extract", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        for entry in &self.libs.manifest.0 {
            match entry {
                ManifestEntry::File { src_relpath, dst } => {
                    trace!("copying {} -> {}", src_relpath.display(), dst.display());
                    copy_dep(&self.libs.libs_dir.join(src_relpath), &ctx.dst_path(dst)?)?;
                }
                ManifestEntry::Symlink { link, target } => {
                    trace!("symlinking {} -> {}", link.display(), target.display());
                    let dst = ctx.dst_path(link)?;
                    let _ = std::fs::remove_file(&dst);
                    std::os::unix::fs::symlink(target, &dst)?;
                }
            }
        }
        Ok(())
    }
}

fn arch_from_elf(elf: &Elf) -> Arch {
    match elf.header.e_machine {
        goblin::elf::header::EM_AARCH64 => Arch::Aarch64,
        // Default to X86_64 for any other value (in practice we only see these two)
        _ => Arch::X86_64,
    }
}

fn expand_rpath(rpath: &str, origin: &Path, arch: Arch) -> PathBuf {
    let lib_dir = match arch {
        Arch::X86_64 => "lib64",
        Arch::Aarch64 => "lib",
    };
    let origin_str = origin.to_string_lossy();
    let expanded = rpath
        .replace("${ORIGIN}", &origin_str)
        .replace("$ORIGIN", &origin_str)
        .replace("${LIB}", lib_dir)
        .replace("$LIB", lib_dir);
    PathBuf::from(expanded)
}

fn with_sysroot(path: &Path, sysroot: Option<&Path>) -> PathBuf {
    match sysroot {
        Some(sr) => sr.join_abs(path),
        None => path.to_path_buf(),
    }
}

/// Returned paths are stored WITHOUT the sysroot prefix.
fn parse_ld_so_conf(conf_path: &Path, sysroot: Option<&Path>) -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    parse_ld_so_conf_file(conf_path, sysroot, &mut dirs);
    dirs
}

fn parse_ld_so_conf_file(path: &Path, sysroot: Option<&Path>, dirs: &mut Vec<PathBuf>) {
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(_) => return,
    };
    let reader = std::io::BufReader::new(file);
    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => continue,
        };
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(pattern) = line
            .strip_prefix("include ")
            .or_else(|| line.strip_prefix("include\t"))
        {
            let pattern = pattern.trim();
            let real_pattern = with_sysroot(Path::new(pattern), sysroot);
            if let Ok(entries) = glob::glob(&real_pattern.to_string_lossy()) {
                for entry in entries.flatten() {
                    parse_ld_so_conf_file(&entry, sysroot, dirs);
                }
            }
        } else {
            dirs.push(PathBuf::from(line));
        }
    }
}

/// Returns path WITHOUT sysroot prefix.
fn resolve_library(
    soname: &str,
    search_dirs: &[PathBuf],
    sysroot: Option<&Path>,
) -> Option<PathBuf> {
    for dir in search_dirs {
        let candidate = dir.join(soname);
        let real_path = with_sysroot(&candidate, sysroot);
        if real_path.exists() {
            return Some(candidate);
        }
    }
    None
}

/// All returned paths are WITHOUT sysroot prefix.
struct DepsCollector<'a> {
    sysroot: Option<&'a Path>,
    ldso_conf_dirs: &'a [PathBuf],
    interpreter_dir: Option<&'a Path>,
    visited: HashSet<PathBuf>,
    result: Vec<PathBuf>,
}

impl<'a> DepsCollector<'a> {
    fn collect(&mut self, binary_path: &Path) -> Result<()> {
        if self.visited.contains(binary_path) {
            return Ok(());
        }
        self.visited.insert(binary_path.to_path_buf());

        let real_path = with_sysroot(binary_path, self.sysroot);
        let buf = std::fs::read(&real_path)
            .with_context(|| format!("while reading {}", real_path.display()))?;
        let elf = Elf::parse(&buf)
            .with_context(|| format!("while parsing ELF {}", binary_path.display()))?;

        self.collect_elf_deps(&elf, binary_path)
    }

    fn collect_elf_deps(&mut self, elf: &Elf, binary_path: &Path) -> Result<()> {
        let arch = arch_from_elf(elf);
        let origin = binary_path.parent().unwrap_or(Path::new("/"));

        let mut search_dirs: Vec<PathBuf> = Vec::new();

        // DT_RUNPATH takes precedence; if empty, fall back to DT_RPATH
        let rpath_entries = if !elf.runpaths.is_empty() {
            &elf.runpaths
        } else {
            &elf.rpaths
        };
        for entry in rpath_entries {
            for component in entry.split(':') {
                if !component.is_empty() {
                    search_dirs.push(expand_rpath(component, origin, arch));
                }
            }
        }

        if let Some(interp_dir) = self.interpreter_dir {
            search_dirs.push(interp_dir.to_path_buf());
        }

        search_dirs.extend_from_slice(self.ldso_conf_dirs);

        // Default search directories are only added when resolving within a
        // sysroot (from_layer). For buck binaries (sysroot=None), the fbcode
        // platform linker's compiled-in system_dirs are just {prefix}/lib/ which
        // is already covered by interpreter_dir above.
        if self.sysroot.is_some() {
            search_dirs.push(PathBuf::from("/lib"));
            search_dirs.push(PathBuf::from("/lib64"));
            search_dirs.push(PathBuf::from("/usr/lib"));
            search_dirs.push(PathBuf::from("/usr/lib64"));
        }

        for needed in &elf.libraries {
            if let Some(resolved) = resolve_library(needed, &search_dirs, self.sysroot) {
                if !self.visited.contains(&resolved) {
                    self.result.push(resolved.clone());
                    self.collect(&resolved)?;
                }
            } else {
                warn!(
                    soname = needed,
                    binary = binary_path.display().to_string(),
                    "could not resolve shared library dependency"
                );
            }
        }

        Ok(())
    }
}

/// Look up absolute paths to all (recursive) deps of this binary
#[tracing::instrument]
pub fn so_dependencies<S: AsRef<OsStr> + std::fmt::Debug>(
    binary: S,
    sysroot: Option<&Path>,
    default_interpreter: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    let binary = Path::new(binary.as_ref());
    let binary_as_seen_from_here = match sysroot {
        Some(sysroot) => Cow::Owned(sysroot.join_abs(binary)),
        None => Cow::Borrowed(binary),
    };

    trace!(
        binary = binary_as_seen_from_here.display().to_string(),
        "reading binary to discover interpreter and dependencies"
    );

    let buf = std::fs::read(&binary_as_seen_from_here)
        .with_context(|| format!("while reading {}", binary_as_seen_from_here.display()))?;
    let elf =
        Elf::parse(&buf).with_context(|| format!("while parsing ELF {}", binary.display()))?;
    let interpreter = elf.interpreter.map_or(default_interpreter, Path::new);

    trace!(
        binary_as_seen_from_here = binary_as_seen_from_here.display().to_string(),
        interpreter = interpreter.display().to_string(),
        "found interpreter"
    );

    // When we have a sysroot (from_layer), use the sysroot's /etc/ld.so.conf.
    // Without a sysroot (buck_binary), use the interpreter's platform-specific
    // ld.so.conf — glibc convention puts it at {prefix}/etc/ld.so.conf where
    // prefix is derived from the interpreter path (e.g.,
    // /usr/local/fbcode/platform010/etc/ld.so.conf for the platform010 linker).
    let ldso_conf_dirs = match sysroot {
        Some(sr) => parse_ld_so_conf(&sr.join("etc/ld.so.conf"), sysroot),
        None => {
            let platform_conf = interpreter
                .parent()
                .and_then(|lib_dir| lib_dir.parent())
                .map(|prefix| prefix.join("etc/ld.so.conf"));
            match platform_conf {
                Some(conf) => parse_ld_so_conf(&conf, None),
                None => Vec::new(),
            }
        }
    };

    let mut collector = DepsCollector {
        sysroot,
        ldso_conf_dirs: &ldso_conf_dirs,
        interpreter_dir: interpreter.parent(),
        visited: HashSet::new(),
        result: Vec::new(),
    };

    // Process the root binary's deps from the already-parsed ELF,
    // avoiding a redundant read+parse.
    collector.visited.insert(binary.to_path_buf());
    collector.collect_elf_deps(&elf, binary)?;

    let interpreter_path = interpreter.to_path_buf();
    if !collector.visited.contains(&interpreter_path) {
        collector.result.push(interpreter_path);
    }

    Ok(collector.result)
}

#[tracing::instrument(err, ret)]
pub fn copy_dep(dep: &Path, dst: &Path) -> Result<()> {
    // create the destination directory tree based on permissions in the source
    if !dst.parent().expect("dst always has parent").exists() {
        for dir in dst
            .parent()
            .expect("dst always has parent")
            .ancestors()
            .collect::<Vec<_>>()
            .into_iter()
            .rev()
        {
            if !dir.exists() {
                trace!("creating parent directory {}", dir.display());
                std::fs::create_dir(dir)?;
            }
        }
    }
    trace!("getting metadata of {}", dep.display());
    let metadata = std::fs::symlink_metadata(dep)
        .with_context(|| format!("while statting '{}'", dep.display()))?;
    trace!("stat of {}: {metadata:?}", dep.display());
    // Thar be dragons. Copying symlinks is probably _never_ what we want - for
    // extracting binaries we want the contents of these dependencies
    let dep: Cow<Path> = if metadata.is_symlink() {
        Cow::Owned(
            std::fs::canonicalize(dep)
                .with_context(|| format!("while canonicalizing symlink dep '{}'", dep.display()))?,
        )
    } else {
        Cow::Borrowed(dep)
    };
    // If the destination file already exists, make sure it's exactly the same
    // as what we're about to copy, to prevent issues like
    // https://fb.workplace.com/groups/btrmeup/posts/5913570682055882
    if dst.exists() &&
    // We don't want to compare against files in /usr/local/fbcode, because the
    // different RE containers these are pulled from might have slightly
    // different versions of the fbcode platform, but the same thing could
    // easily happen for builds so just let it slide.
    !dep.display().to_string().contains("/usr/local/fbcode/")
    {
        let dst_contents = std::fs::read(dst)
            .with_context(|| format!("while reading already-installed '{}'", dst.display()))?;
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(&dst_contents);
        let pre_existing_hash = hasher.finish();

        let src_contents = std::fs::read(&dep)
            .with_context(|| format!("while reading potentially new dep '{}'", dep.display()))?;
        let mut hasher = XxHash64::with_seed(0);
        hasher.write(&src_contents);
        let new_src_hash = hasher.finish();

        trace!(
            "hashed {} (existing = {}, new = {})",
            dst.display(),
            pre_existing_hash,
            new_src_hash
        );

        if pre_existing_hash != new_src_hash {
            return Err(anyhow::anyhow!(
                "extract conflicts with existing file at {}",
                dst.display()
            ));
        }
    } else {
        copy_with_metadata().src(&dep).dst(dst).call()?
    }
    Ok(())
}

pub fn default_interpreter(target: Arch) -> &'static Path {
    Path::new(match target {
        Arch::X86_64 => "/usr/lib64/ld-linux-x86-64.so.2",
        Arch::Aarch64 => "/lib/ld-linux-aarch64.so.1",
    })
}

/// In all the cases that we care about, a library will live under /lib64, but
/// this directory will be a symlink to /usr/lib64. To avoid build conflicts with
/// other image layers, replace it.
pub fn ensure_usr<'a>(path: &'a Path) -> Cow<'a, Path> {
    match path.starts_with("/lib") || path.starts_with("/lib64") {
        false => Cow::Borrowed(path),
        true => Cow::Owned(Path::new("/usr").join_abs(path)),
    }
}
