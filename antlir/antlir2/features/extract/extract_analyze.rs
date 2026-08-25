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
use std::io::BufRead;
use std::io::BufWriter;
use std::path::Path;
use std::path::PathBuf;

use antlir2_compile::Arch;
use antlir2_path::PathExt;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use extract::Manifest;
use extract::ManifestEntry;
use goblin::elf::Elf;
use json_arg::Json;
use regex::Regex;
use serde::Deserialize;
use serde_with::DisplayFromStr;
use serde_with::serde_as;
use tracing::debug;
use tracing::trace;
use tracing::warn;

#[derive(Debug, Parser)]
struct Args {
    #[clap(subcommand)]
    command: Subcommand,
}

#[derive(Debug, clap::Subcommand)]
enum Subcommand {
    BuckBinary(BuckBinaryArgs),
    FromLayer(FromLayerArgs),
}

#[derive(Debug, Parser)]
struct BuckBinaryArgs {
    #[clap(long)]
    src: PathBuf,
    #[clap(long)]
    dst: PathBuf,
    #[clap(long)]
    target_arch: Arch,
    #[clap(long)]
    manifest: PathBuf,
    #[clap(long)]
    libs_dir: PathBuf,
    #[clap(flatten)]
    dlopen_args: DlopenArgs,
}

#[serde_as]
#[derive(Debug, Clone, Deserialize)]
struct FeaturesAllowlist(#[serde_as(as = "Vec<(DisplayFromStr, _)>")] Vec<(Regex, Vec<String>)>);

#[derive(Debug, clap::Args)]
struct DlopenArgs {
    #[clap(long, default_value_t)]
    dlopen_min_priority: DlopenPriority,
    #[clap(long, default_value = "[]")]
    dlopen_features_allow: Json<FeaturesAllowlist>,
    #[clap(long, default_value = "[]")]
    dlopen_features_deny: Json<FeaturesAllowlist>,
}

#[derive(Debug, Parser)]
struct FromLayerArgs {
    #[clap(long)]
    layer: PathBuf,
    #[clap(long = "binary")]
    binaries: Vec<PathBuf>,
    #[clap(long)]
    target_arch: Arch,
    #[clap(long)]
    manifest: PathBuf,
    #[clap(long)]
    libs_dir: PathBuf,
    #[clap(flatten)]
    dlopen_args: DlopenArgs,
}

fn write_manifest(entries: BTreeSet<ManifestEntry>, path: &Path) -> Result<()> {
    let f = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(f, &Manifest(entries))?;
    Ok(())
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
    dlopen_filter: &'a DlopenFilter,
}

const NT_FDO_DLOPEN: u32 = 0x407c0c0a;

#[derive(
    Debug,
    Clone,
    Copy,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Default,
    Deserialize,
    strum::Display,
    clap::ValueEnum
)]
#[serde(rename_all = "lowercase")]
/// The declared priority for a dlopen note dep
///
/// These are the only legal values according to the doc, so rejecting anything
/// outside of this is correct behavior
enum DlopenPriority {
    /// Core functionality needs the dependency, the binary will not work if it
    /// cannot be found
    Suggested,
    /// Important functionality needs the dependency, the binary will work but
    /// in most cases the dependency should be provided
    #[default]
    Recommended,
    /// Core functionality needs the dependency, the binary will not work if it
    /// cannot be found
    Required,
}

#[derive(Debug, Clone, Default)]
pub(crate) struct DlopenFilter {
    min_priority: DlopenPriority,
    allow: Vec<(Regex, Vec<String>)>,
    deny: Vec<(Regex, Vec<String>)>,
}

impl DlopenFilter {
    fn from_args(args: DlopenArgs) -> Result<Self> {
        Ok(Self {
            min_priority: args.dlopen_min_priority,
            allow: args.dlopen_features_allow.into_inner().0,
            deny: args.dlopen_features_deny.into_inner().0,
        })
    }

    fn allowed_features_for(&self, binary_path: &Path) -> HashSet<String> {
        let path_str = binary_path.to_string_lossy();
        let mut allowed = HashSet::new();
        for (pattern, features) in &self.allow {
            if pattern.is_match(&path_str) {
                allowed.extend(features.iter().cloned());
            }
        }
        allowed
    }

    fn denied_features_for(&self, binary_path: &Path) -> HashSet<String> {
        let path_str = binary_path.to_string_lossy();
        let mut denied = HashSet::new();
        for (pattern, features) in &self.deny {
            if pattern.is_match(&path_str) {
                denied.extend(features.iter().cloned());
            }
        }
        denied
    }
}

#[derive(Debug, Deserialize)]
struct DlopenNoteEntry {
    #[serde(default)]
    soname: Vec<String>,
    #[serde(default)]
    priority: Option<DlopenPriority>,
    #[serde(default)]
    feature: Option<String>,
}

fn dlopen_entries_from_desc(desc: &[u8]) -> Vec<DlopenNoteEntry> {
    let s = String::from_utf8_lossy(desc);
    let trimmed = s.trim_matches(|c: char| c == '\0').trim();
    if trimmed.is_empty() {
        return Vec::new();
    }
    match serde_json::from_str::<Vec<DlopenNoteEntry>>(trimmed) {
        Ok(entries) => entries,
        Err(e) => {
            warn!("failed to parse .note.dlopen JSON payload: {}", e);
            Vec::new()
        }
    }
}

fn filter_dlopen_entries(
    entries: Vec<DlopenNoteEntry>,
    filter: &DlopenFilter,
    binary_path: &Path,
) -> Vec<String> {
    let allowed = filter.allowed_features_for(binary_path);
    let denied = filter.denied_features_for(binary_path);

    let mut sonames = Vec::new();
    for entry in entries {
        // Deny takes precedence: if feature matches denylist, skip
        if let Some(ref feat) = entry.feature {
            if denied.contains(feat) {
                continue;
            }
        }

        // Allowlist is union + precedence over priority
        if let Some(ref feat) = entry.feature {
            if allowed.contains(feat) {
                sonames.extend(entry.soname);
                continue;
            }
        }

        // Fall back to priority filtering
        let Some(priority) = entry.priority else {
            continue;
        };
        if priority < filter.min_priority {
            continue;
        }
        sonames.extend(entry.soname);
    }
    sonames
}

fn extract_dlopen_sonames(
    elf_data: &[u8],
    elf: &Elf,
    filter: &DlopenFilter,
    binary_path: &Path,
) -> Result<Vec<String>> {
    let mut all_entries = Vec::new();

    let mut iters = Vec::new();
    if let Some(it) = elf.iter_note_headers(elf_data) {
        iters.push(it);
    }
    if let Some(it) = elf.iter_note_sections(elf_data, None) {
        iters.push(it);
    }

    for iter in iters {
        for note_res in iter {
            if let Ok(note) = note_res {
                if note.n_type == NT_FDO_DLOPEN && note.name == "FDO" {
                    all_entries.extend(dlopen_entries_from_desc(note.desc));
                }
            }
        }
    }

    Ok(filter_dlopen_entries(all_entries, filter, binary_path))
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

        self.collect_elf_deps(&elf, binary_path, &buf)
    }

    fn collect_elf_deps(&mut self, elf: &Elf, binary_path: &Path, elf_data: &[u8]) -> Result<()> {
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

        for dlopen_soname in extract_dlopen_sonames(elf_data, elf, self.dlopen_filter, binary_path)?
        {
            if let Some(resolved) = resolve_library(&dlopen_soname, &search_dirs, self.sysroot) {
                if !self.visited.contains(&resolved) {
                    self.result.push(resolved.clone());
                    self.collect(&resolved)?;
                }
            } else {
                warn!(
                    soname = dlopen_soname,
                    binary = binary_path.display().to_string(),
                    "could not resolve dlopen shared library dependency"
                );
            }
        }

        Ok(())
    }
}

/// Look up absolute paths to all (recursive) deps of this binary
#[tracing::instrument]
pub(crate) fn so_dependencies<S: AsRef<OsStr> + std::fmt::Debug>(
    binary: S,
    sysroot: Option<&Path>,
    default_interpreter: &Path,
    dlopen_filter: &DlopenFilter,
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
        dlopen_filter,
    };

    // Process the root binary's deps from the already-parsed ELF,
    // avoiding a redundant read+parse.
    collector.visited.insert(binary.to_path_buf());
    collector.collect_elf_deps(&elf, binary, &buf)?;

    let interpreter_path = interpreter.to_path_buf();
    if !collector.visited.contains(&interpreter_path) {
        collector.result.push(interpreter_path);
    }

    Ok(collector.result)
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

// ---------------------------------------------------------------------------
// Binary entrypoint (analysis actions)
// ---------------------------------------------------------------------------

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();
    let args = Args::parse();

    match args.command {
        Subcommand::BuckBinary(args) => buck_binary(args),
        Subcommand::FromLayer(args) => from_layer(args),
    }
}

fn buck_binary(args: BuckBinaryArgs) -> Result<()> {
    let default_interp = default_interpreter(args.target_arch);
    let src = args.src.canonicalize()?;
    let dlopen_filter = DlopenFilter::from_args(args.dlopen_args)?;
    let deps = so_dependencies(src.clone(), None, default_interp, &dlopen_filter)?;

    let mut entries = BTreeSet::new();

    std::fs::create_dir_all(&args.libs_dir)?;
    let main_relpath = PathBuf::from("__main");
    std::fs::copy(&src, args.libs_dir.join(&main_relpath))
        .with_context(|| format!("while copying {}", src.display()))?;
    entries.insert(ManifestEntry::File {
        src_relpath: main_relpath,
        dst: args.dst.clone(),
    });

    for dep in &deps {
        let (src_relpath, dst) =
            match dep.strip_prefix(src.parent().expect("src always has parent")) {
                Ok(relpath) => {
                    debug!(
                        relpath = relpath.display().to_string(),
                        "installing library at path relative to dst"
                    );
                    let src_relpath =
                        Path::new("__relative").join(relpath.strip_prefix("..").unwrap_or(relpath));
                    let abs_dst = args
                        .dst
                        .parent()
                        .expect("dst always has parent")
                        .join(relpath);
                    (src_relpath, abs_dst)
                }
                Err(_) => {
                    let src_relpath = dep
                        .strip_prefix("/")
                        .expect("non-relative libs are absolute");
                    (src_relpath.to_owned(), dep.to_owned())
                }
            };

        let copy_path = args.libs_dir.join(&src_relpath);
        std::fs::create_dir_all(copy_path.parent().expect("always has parent"))?;
        std::fs::copy(dep, &copy_path).with_context(|| {
            format!("while copying {} to {}", dep.display(), copy_path.display())
        })?;

        entries.insert(ManifestEntry::File { src_relpath, dst });
    }

    write_manifest(entries, &args.manifest)
}

fn from_layer(args: FromLayerArgs) -> Result<()> {
    let default_interp = default_interpreter(args.target_arch);
    let src_layer = args
        .layer
        .canonicalize()
        .context("while looking up abspath of src layer")?;
    let dlopen_filter = DlopenFilter::from_args(args.dlopen_args)?;

    let mut entries = BTreeSet::new();
    let mut added_relpaths = HashSet::new();
    let mut all_deps = BTreeSet::new();

    for binary in &args.binaries {
        let src = src_layer.join_abs(binary);

        let src_meta = std::fs::symlink_metadata(&src)
            .with_context(|| format!("while lstatting {}", src.display()))?;

        let real_binary = if src_meta.is_symlink() {
            let canonical_target = src
                .canonicalize()
                .with_context(|| format!("while canonicalizing {}", src.display()))?;

            if canonical_target
                .components()
                .any(|c| c.as_os_str() == OsStr::new("buck-out"))
            {
                anyhow::bail!(
                    "{} looks like a buck-built binary ({}). You must use \
                     feature.extract_buck_binary instead",
                    src.display(),
                    canonical_target.display(),
                );
            }

            let canonical_target_rel = canonical_target
                .strip_prefix(&src_layer)
                .unwrap_or(canonical_target.as_path());
            let target_under_src = src_layer.join(
                canonical_target_rel
                    .strip_prefix("/")
                    .unwrap_or(canonical_target.as_path()),
            );
            if !target_under_src.exists() {
                anyhow::bail!(
                    "symlink target {} ({} under src_layer) does not actually exist",
                    canonical_target.display(),
                    target_under_src.display(),
                );
            }

            let src_relpath = canonical_target_rel.strip_abs().to_path_buf();
            if added_relpaths.insert(src_relpath.clone()) {
                let copy_path = args.libs_dir.join(&src_relpath);
                std::fs::create_dir_all(copy_path.parent().expect("always has parent"))?;
                std::fs::copy(&target_under_src, &copy_path)?;
                entries.insert(ManifestEntry::File {
                    src_relpath,
                    dst: canonical_target_rel.to_owned(),
                });
            }

            let target = std::fs::read_link(&src)
                .with_context(|| format!("while reading the link target of {}", src.display()))?;
            entries.insert(ManifestEntry::Symlink {
                link: binary.to_owned(),
                target,
            });

            canonical_target
        } else {
            let src_relpath = binary.strip_abs().to_path_buf();
            if added_relpaths.insert(src_relpath.clone()) {
                let copy_path = args.libs_dir.join(&src_relpath);
                std::fs::create_dir_all(copy_path.parent().expect("always has parent"))?;
                std::fs::copy(&src, &copy_path)?;
                entries.insert(ManifestEntry::File {
                    src_relpath,
                    dst: binary.to_owned(),
                });
            }

            binary.to_owned()
        };

        let real_binary_in_layer = real_binary
            .strip_prefix(&src_layer)
            .unwrap_or(real_binary.as_path());
        all_deps.extend(
            so_dependencies(
                real_binary_in_layer,
                Some(&src_layer),
                default_interp,
                &dlopen_filter,
            )?
            .into_iter()
            .map(|path| ensure_usr(&path).to_path_buf()),
        );
    }

    let cwd = std::env::current_dir()?;
    for dep in all_deps {
        let src_relpath = dep.strip_abs().to_path_buf();
        if !added_relpaths.insert(src_relpath.clone()) {
            continue; // already added as a binary
        }

        let path_in_src_layer = src_layer.join_abs(&dep);
        // If the dep path within the container is under the current
        // cwd (aka, the repo), we need to get the file out of the
        // host instead of the container.
        let dep_copy_path = if dep.starts_with(&cwd) {
            // As a good safety check, we also ensure that the file
            // does not exist inside the container, to prevent any
            // unintended extractions from the build host's
            // non-deterministic environment.
            if path_in_src_layer.exists() {
                anyhow::bail!(
                    "'{}' exists but it seems like we should get it from the host",
                    path_in_src_layer.display(),
                );
            }
            dep.clone()
        } else {
            path_in_src_layer
        };

        let copy_path = args.libs_dir.join(&src_relpath);
        std::fs::create_dir_all(copy_path.parent().expect("always has parent"))?;
        // Follow symlinks - we always want the actual file contents
        let resolved = if dep_copy_path.is_symlink() {
            dep_copy_path
                .canonicalize()
                .with_context(|| format!("while canonicalizing {}", dep_copy_path.display()))?
        } else {
            dep_copy_path
        };
        std::fs::copy(&resolved, &copy_path).with_context(|| {
            format!(
                "while copying {} to {}",
                resolved.display(),
                copy_path.display()
            )
        })?;

        entries.insert(ManifestEntry::File {
            src_relpath,
            dst: dep.to_owned(),
        });
    }

    write_manifest(entries, &args.manifest)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(
        soname: &str,
        priority: Option<DlopenPriority>,
        feature: Option<&str>,
    ) -> DlopenNoteEntry {
        DlopenNoteEntry {
            soname: vec![soname.to_string()],
            priority,
            feature: feature.map(|f| f.to_string()),
        }
    }

    fn filter_for(
        min_priority: DlopenPriority,
        allow: Vec<(&str, Vec<&str>)>,
        deny: Vec<(&str, Vec<&str>)>,
    ) -> DlopenFilter {
        let allow = allow
            .into_iter()
            .map(|(re, feats)| {
                (
                    Regex::new(re).expect("valid regex"),
                    feats.into_iter().map(|s| s.to_string()).collect(),
                )
            })
            .collect();
        let deny = deny
            .into_iter()
            .map(|(re, feats)| {
                (
                    Regex::new(re).expect("valid regex"),
                    feats.into_iter().map(|s| s.to_string()).collect(),
                )
            })
            .collect();
        DlopenFilter {
            min_priority,
            allow,
            deny,
        }
    }

    #[test]
    fn test_priority_filtering_default() {
        let entries = vec![
            make_entry(
                "libz.so.1",
                Some(DlopenPriority::Required),
                Some("prio-required"),
            ),
            make_entry(
                "libbz2.so.1",
                Some(DlopenPriority::Recommended),
                Some("prio-recommended"),
            ),
            make_entry(
                "liblzma.so.5",
                Some(DlopenPriority::Suggested),
                Some("prio-suggested"),
            ),
        ];
        let filter = filter_for(DlopenPriority::Recommended, vec![], vec![]);
        let binary_path = Path::new("/usr/bin/test");
        let result = filter_dlopen_entries(entries, &filter, binary_path);
        assert!(result.contains(&"libz.so.1".to_string()));
        assert!(result.contains(&"libbz2.so.1".to_string()));
        assert!(!result.contains(&"liblzma.so.5".to_string()));
    }

    #[test]
    fn test_allow_overrides_priority() {
        // min_priority required, but allow suggested via .*
        let entries = vec![
            make_entry(
                "libz.so.1",
                Some(DlopenPriority::Required),
                Some("prio-required"),
            ),
            make_entry(
                "liblzma.so.5",
                Some(DlopenPriority::Suggested),
                Some("prio-suggested"),
            ),
        ];
        let filter = filter_for(
            DlopenPriority::Required,
            vec![(".*", vec!["prio-suggested"])],
            vec![],
        );
        let binary_path = Path::new("/some/path/dlopen_binary");
        let result = filter_dlopen_entries(entries, &filter, binary_path);
        assert!(result.contains(&"libz.so.1".to_string()));
        assert!(
            result.contains(&"liblzma.so.5".to_string()),
            "allowlist should include suggested despite required filter"
        );
    }

    #[test]
    fn test_allow_specific_regex_matching() {
        let entries = vec![
            make_entry(
                "libz.so.1",
                Some(DlopenPriority::Required),
                Some("prio-required"),
            ),
            make_entry(
                "libcap.so.2",
                Some(DlopenPriority::Recommended),
                Some("feature-a"),
            ),
        ];
        let filter = filter_for(
            DlopenPriority::Required,
            vec![("dlopen_binary", vec!["feature-a"])],
            vec![],
        );
        let binary_path = Path::new("/buck-out/v2/gen/fbcode/antlir/dlopen_binary");
        let result = filter_dlopen_entries(entries, &filter, binary_path);
        assert!(result.contains(&"libz.so.1".to_string()));
        assert!(result.contains(&"libcap.so.2".to_string()));
    }

    #[test]
    fn test_allow_union_multiple_regexes() {
        let entries = vec![
            make_entry(
                "libz.so.1",
                Some(DlopenPriority::Required),
                Some("prio-required"),
            ),
            make_entry(
                "libcap.so.2",
                Some(DlopenPriority::Recommended),
                Some("feature-a"),
            ),
            make_entry(
                "libffi.so.6",
                Some(DlopenPriority::Recommended),
                Some("feature-b"),
            ),
        ];
        let filter = filter_for(
            DlopenPriority::Required,
            vec![("dlopen", vec!["feature-a"]), ("binary", vec!["feature-b"])],
            vec![],
        );
        let binary_path = Path::new("/path/to/dlopen_binary");
        let result = filter_dlopen_entries(entries, &filter, binary_path);
        assert!(result.contains(&"libz.so.1".to_string()));
        assert!(result.contains(&"libcap.so.2".to_string()));
        assert!(result.contains(&"libffi.so.6".to_string()));
    }

    #[test]
    fn test_deny_overrides_priority() {
        let entries = vec![
            make_entry(
                "libz.so.1",
                Some(DlopenPriority::Required),
                Some("prio-required"),
            ),
            make_entry(
                "libbz2.so.1",
                Some(DlopenPriority::Recommended),
                Some("feature-b"),
            ),
            make_entry(
                "libcap.so.2",
                Some(DlopenPriority::Recommended),
                Some("feature-a"),
            ),
        ];
        let filter = filter_for(
            DlopenPriority::Recommended,
            vec![],
            vec![(".*", vec!["feature-b"])],
        );
        let binary_path = Path::new("/usr/bin/test");
        let result = filter_dlopen_entries(entries, &filter, binary_path);
        assert!(result.contains(&"libz.so.1".to_string()));
        assert!(result.contains(&"libcap.so.2".to_string()));
        assert!(
            !result.contains(&"libbz2.so.1".to_string()),
            "denylist should exclude even when priority matches"
        );
    }

    #[test]
    fn test_deny_precedence_over_allow() {
        let entries = vec![
            make_entry(
                "libz.so.1",
                Some(DlopenPriority::Required),
                Some("prio-required"),
            ),
            make_entry(
                "libcap.so.2",
                Some(DlopenPriority::Recommended),
                Some("feature-a"),
            ),
            make_entry(
                "liblzma.so.5",
                Some(DlopenPriority::Suggested),
                Some("prio-suggested"),
            ),
        ];
        let filter = filter_for(
            DlopenPriority::Required,
            vec![(".*", vec!["feature-a", "prio-suggested"])],
            vec![(".*", vec!["feature-a"])],
        );
        let binary_path = Path::new("/path/dlopen_binary");
        let result = filter_dlopen_entries(entries, &filter, binary_path);
        assert!(result.contains(&"libz.so.1".to_string()));
        assert!(result.contains(&"liblzma.so.5".to_string()));
        assert!(
            !result.contains(&"libcap.so.2".to_string()),
            "deny should take precedence over allow"
        );
    }

    #[test]
    fn test_allowed_denied_features_for_union() {
        let filter = filter_for(
            DlopenPriority::Recommended,
            vec![
                ("bin_a", vec!["feature-a"]),
                ("bin", vec!["feature-b"]),
                (".*", vec!["feature-c"]),
            ],
            vec![(".*", vec!["denied-1"]), ("specific", vec!["denied-2"])],
        );
        // binary path matches "bin", "bin_a", and ".*" for allow
        let binary_path = Path::new("/usr/bin/bin_a_test");
        let allowed = filter.allowed_features_for(binary_path);
        assert!(allowed.contains("feature-a"));
        assert!(allowed.contains("feature-b"));
        assert!(allowed.contains("feature-c"));

        // denied: .* matches, specific does not match this path unless contains "specific"
        let denied = filter.denied_features_for(binary_path);
        assert!(denied.contains("denied-1"));
        assert!(!denied.contains("denied-2"));

        let binary_path2 = Path::new("/specific/path");
        let denied2 = filter.denied_features_for(binary_path2);
        assert!(denied2.contains("denied-1"));
        assert!(denied2.contains("denied-2"));
    }

    #[test]
    fn test_entry_without_feature_uses_priority_only() {
        // Entry without feature should be filtered only by priority, not allow/deny
        let entries = vec![
            make_entry("libz.so.1", Some(DlopenPriority::Required), None),
            make_entry("libbz2.so.1", Some(DlopenPriority::Recommended), None),
        ];
        let filter = filter_for(
            DlopenPriority::Required,
            vec![(".*", vec!["some-feature"])],
            vec![(".*", vec!["other"])],
        );
        let binary_path = Path::new("/any");
        let result = filter_dlopen_entries(entries, &filter, binary_path);
        assert!(result.contains(&"libz.so.1".to_string()));
        assert!(!result.contains(&"libbz2.so.1".to_string()));
    }
}
