/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::hash::Hasher;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_compile::util::copy_with_metadata;
use antlir2_compile::Arch;
use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::FileType;
use antlir2_depgraph::item::FsEntry;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::item::ItemKey;
use antlir2_depgraph::item::Path as PathItem;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_depgraph::requires_provides::Validator;
use antlir2_features::types::BuckOutSource;
use antlir2_features::types::LayerInfo;
use antlir2_features::types::PathInLayer;
use antlir2_isolate::isolate;
use antlir2_isolate::IsolationContext;
use anyhow::Context;
use anyhow::Result;
use goblin::elf::Elf;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::de::Error;
use serde::Deserialize;
use serde::Serialize;
use tracing::trace;
use tracing::warn;
use twox_hash::XxHash64;

pub type Feature = Extract;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Extract {
    Buck(ExtractBuckBinary),
    Layer(ExtractLayerBinaries),
}

/// Buck2's `record` will always include `null` values, but serde's native enum
/// deserialization will fail if there are multiple keys, even if the others are
/// null.
/// TODO(vmagro): make this general in the future (either codegen from `record`s
/// or as a proc-macro)
impl<'de> Deserialize<'de> for Extract {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct ExtractStruct {
            buck: Option<ExtractBuckBinary>,
            layer: Option<ExtractLayerBinaries>,
        }

        ExtractStruct::deserialize(deserializer).and_then(|s| match (s.buck, s.layer) {
            (Some(v), None) => Ok(Self::Buck(v)),
            (None, Some(v)) => Ok(Self::Layer(v)),
            (_, _) => Err(D::Error::custom("exactly one of {buck, layer} must be set")),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct ExtractBuckBinary {
    pub src: BuckOutSource,
    pub dst: PathInLayer,
}

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct ExtractLayerBinaries {
    pub layer: LayerInfo,
    pub binaries: Vec<PathInLayer>,
}

impl antlir2_depgraph::requires_provides::RequiresProvides for Extract {
    fn provides(&self) -> Result<Vec<Item<'static>>, String> {
        // Intentionally provide only the direct files the user asked for,
        // because we don't want to produce conflicts with all the transitive
        // dependencies. However, we will check that any duplicated items are in
        // fact identical, to prevent insane mismatches like this
        // https://fb.workplace.com/groups/btrmeup/posts/5913570682055882
        Ok(match self {
            Self::Layer(l) => l
                .binaries
                .iter()
                .map(|path| {
                    Item::Path(PathItem::Entry(FsEntry {
                        path: path.to_owned().into(),
                        file_type: FileType::File,
                        mode: 0o555,
                    }))
                })
                .collect(),
            Self::Buck(b) => {
                vec![Item::Path(PathItem::Entry(FsEntry {
                    path: b.dst.to_owned().into(),
                    file_type: FileType::File,
                    mode: 0o555,
                }))]
            }
        })
    }

    fn requires(&self) -> Result<Vec<Requirement<'static>>, String> {
        Ok(match self {
            Self::Layer(l) => l
                .binaries
                .iter()
                .flat_map(|path| {
                    vec![
                        Requirement::ordered(
                            ItemKey::Layer(l.layer.label.to_owned()),
                            Validator::ItemInLayer {
                                key: ItemKey::Path(path.to_owned().into()),
                                // TODO(T153458901): for correctness, this
                                // should be Validator::Executable, but some
                                // depgraph validation is currently buggy and
                                // produces false negatives
                                validator: Box::new(Validator::Exists),
                            },
                        ),
                        Requirement::ordered(
                            ItemKey::Path(
                                path.parent()
                                    .expect("dst always has parent")
                                    .to_owned()
                                    .into(),
                            ),
                            Validator::FileType(FileType::Directory),
                        ),
                    ]
                })
                .collect(),
            Self::Buck(b) => vec![Requirement::ordered(
                ItemKey::Path(
                    b.dst
                        .parent()
                        .expect("dst always has parent")
                        .to_owned()
                        .into(),
                ),
                Validator::FileType(FileType::Directory),
            )],
        })
    }
}

impl antlir2_compile::CompileFeature for Extract {
    #[tracing::instrument(name = "extract", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        let default_interpreter = Path::new(match ctx.target_arch() {
            Arch::X86_64 => "/usr/lib64/ld-linux-x86-64.so.2",
            Arch::Aarch64 => "/lib/ld-linux-aarch64.so.1",
        });
        match self {
            Self::Buck(buck) => {
                let src = buck.src.canonicalize()?;
                let deps = so_dependencies(buck.src.to_owned(), None, default_interpreter)?;
                for dep in &deps {
                    if let Ok(relpath) =
                        dep.strip_prefix(src.parent().expect("src always has parent"))
                    {
                        tracing::debug!(
                            relpath = relpath.display().to_string(),
                            "installing library at path relative to dst"
                        );
                        copy_dep(
                            dep,
                            &ctx.dst_path(
                                &buck
                                    .dst
                                    .parent()
                                    .expect("dst always has parent")
                                    .join(relpath),
                            )?,
                        )?;
                    } else {
                        copy_dep(dep, &ctx.dst_path(dep.strip_prefix("/").unwrap_or(dep))?)?;
                    }
                }
                // don't copy the metadata from the buck binary, the owner will
                // be wrong
                trace!(
                    "copying {} -> {}",
                    buck.src.display(),
                    ctx.dst_path(&buck.dst)?.display()
                );
                std::fs::copy(&buck.src, ctx.dst_path(&buck.dst)?)?;
                Ok(())
            }
            Self::Layer(layer) => {
                let src_layer = layer
                    .layer
                    .subvol_symlink
                    .canonicalize()
                    .context("while looking up abspath of src layer")?;
                trace!("extract root = {}", src_layer.display());
                let mut all_deps = HashSet::new();
                for binary in &layer.binaries {
                    let src = src_layer.join(binary.strip_prefix("/").unwrap_or(binary));
                    let dst = ctx.dst_path(binary)?;

                    let src_meta = std::fs::symlink_metadata(&src)
                        .with_context(|| format!("while lstatting {}", src.display()))?;
                    let real_src = if src_meta.is_symlink() {
                        // If src is a symlink, the destination should also be
                        // created as a symlink, and the target should be
                        // processed as the real binary.

                        let canonical_target = src
                            .canonicalize()
                            .with_context(|| format!("while canonicalizing {}", src.display()))?;

                        if canonical_target
                            .components()
                            .any(|c| c.as_os_str() == OsStr::new("buck-out"))
                        {
                            warn!(
                                "{} looks like a buck-built binary ({}). You should use feature.extract_buck_binary",
                                src.display(),
                                canonical_target.display(),
                            );
                            Self::Buck(ExtractBuckBinary {
                                src: canonical_target.clone(),
                                dst: binary.to_owned(),
                            })
                            .compile(ctx)
                            .with_context(|| {
                                format!(
                                    "while extracting buck binary '{}'",
                                    canonical_target.display()
                                )
                            })?;
                            continue;
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
                            return Err(anyhow::anyhow!(
                                "symlink target {} ({} under src_layer) does not actually exist",
                                canonical_target.display(),
                                target_under_src.display()
                            )
                            .into());
                        }

                        copy_with_metadata(
                            &target_under_src,
                            &ctx.dst_path(canonical_target_rel)?,
                            None,
                            None,
                        )
                        .context("while copying target_under_src to canonical_target_rel")?;

                        // use the exact same link target when recreating the
                        // symlinkg (in other words, the same "relativeness")
                        let target = std::fs::read_link(&src).with_context(|| {
                            format!("while reading the link target of  {}", src.display())
                        })?;

                        std::os::unix::fs::symlink(&target, &dst).with_context(|| {
                            format!("while symlinking {} -> {}", dst.display(), target.display())
                        })?;

                        canonical_target
                    } else {
                        // if the binary is a regular file, copy it directly
                        copy_with_metadata(&src, &dst, None, None)?;
                        binary.to_owned()
                    };

                    all_deps.extend(
                        so_dependencies(
                            real_src
                                .strip_prefix(&src_layer)
                                .unwrap_or(real_src.as_path()),
                            Some(&src_layer),
                            default_interpreter,
                        )?
                        .into_iter()
                        .map(|path| ensure_usr(&path).to_path_buf()),
                    );
                }
                let cwd = std::env::current_dir()?;
                for dep in all_deps {
                    let path_in_src_layer = src_layer.join(dep.strip_prefix("/").unwrap_or(&dep));
                    // If the dep path within the container is under the current
                    // cwd (aka, the repo), we need to get the file out of the
                    // host instead of the container.
                    let dep_copy_path = if dep.starts_with(&cwd) {
                        // As a good safety check, we also ensure that the file
                        // does not exist inside the container, to prevent any
                        // unintended extractions from the build host's
                        // non-deterministic environment. This check should
                        // never pass unless something about the build
                        // environment setup wildly changes, so we should return
                        // an error immediately in case it does.
                        if path_in_src_layer.exists() {
                            return Err(anyhow::anyhow!(
                                "'{}' exists but it seems like we should get it from the host",
                                path_in_src_layer.display()
                            )
                            .into());
                        }
                        dep.clone()
                    } else {
                        path_in_src_layer
                    };
                    copy_dep(&dep_copy_path, &ctx.dst_path(&dep)?)?;
                }
                Ok(())
            }
        }
    }
}

/// Simple regex to parse the output of `ld.so --list` which is used to resolve
/// the dependencies of a binary.
static LDSO_RE: Lazy<Regex> = Lazy::new(|| {
    regex::RegexBuilder::new(r"^\s*(?P<name>.+)\s+=>\s+(?P<path>.+)\s+\(0x[0-9a-f]+\)$")
        .multi_line(true)
        .build()
        .expect("this is a valid regex")
});

/// In all the cases that we care about, a library will live under /lib64, but
/// this directory will be a symlink to /usr/lib64. To avoid build conflicts with
/// other image layers, replace it.
fn ensure_usr<'a>(path: &'a Path) -> Cow<'a, Path> {
    match path.starts_with("/lib") || path.starts_with("/lib64") {
        false => Cow::Borrowed(path),
        true => Cow::Owned(Path::new("/usr").join(path.strip_prefix("/").unwrap_or(path))),
    }
}

/// Look up absolute paths to all (recursive) deps of this binary
#[tracing::instrument]
fn so_dependencies<S: AsRef<OsStr> + std::fmt::Debug>(
    binary: S,
    sysroot: Option<&Path>,
    default_interpreter: &Path,
) -> anyhow::Result<Vec<PathBuf>> {
    let binary = Path::new(binary.as_ref());
    let binary_as_seen_from_here = match sysroot {
        Some(sysroot) => Cow::Owned(sysroot.join(binary.strip_prefix("/").unwrap_or(binary))),
        None => Cow::Borrowed(binary),
    };

    trace!(
        binary = binary_as_seen_from_here.display().to_string(),
        "reading binary to discover interpreter"
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

    let mut cmd = Command::new(interpreter);
    if let Some(sysroot) = sysroot {
        let cwd = std::env::current_dir()?;
        cmd = isolate(
            IsolationContext::builder(sysroot)
                .ephemeral(true)
                .platform([
                    cwd.as_path(),
                    #[cfg(facebook)]
                    Path::new("/usr/local/fbcode"),
                    #[cfg(facebook)]
                    Path::new("/mnt/gvfs"),
                ])
                .working_directory(&cwd)
                // There's a memory allocation bug under qemu-aarch64 when asking the linker to --list
                // an elf binary.  This configures qemu-aarch64 to pre-allocate enough virtual address
                // space to not exploded in this case.  This env var has no effect when running on the
                // native host (x86_64 or aarch64).
                // TODO: Remove this after the issue is found and fixed with qemu-aarch64.
                .setenv(("QEMU_RESERVED_VA", "0x40000000"))
                .build(),
        )?
        .command(interpreter)?;
    } else {
        cmd.env("QEMU_RESERVED_VA", "0x40000000");
    }

    // Canonicalize the binary path before dealing with ld.so, because that does
    // not correctly handle relative rpaths coming via symlinks (which we will
    // see in @mode/dev binaries).
    let binary = binary_as_seen_from_here.canonicalize().with_context(|| {
        format!(
            "while getting abspath of binary '{}'",
            binary_as_seen_from_here.display()
        )
    })?;

    cmd.arg("--list").arg(&binary);

    trace!("running ld.so {cmd:?}");

    let output = cmd
        .output()
        .with_context(|| format!("while listing libraries for {:?}", binary))?;
    anyhow::ensure!(
        output.status.success(),
        "{} --list {} failed with exit code {}: {}\n{}",
        interpreter.display(),
        binary.display(),
        output.status,
        std::str::from_utf8(&output.stdout).unwrap_or("<not utf8>"),
        std::str::from_utf8(&output.stderr).unwrap_or("<not utf8>"),
    );
    let ld_output_str = std::str::from_utf8(&output.stdout).context("ld.so output not utf-8")?;

    Ok(LDSO_RE
        .captures_iter(ld_output_str)
        .map(|cap| {
            let path = Path::new(
                cap.name("path")
                    .expect("must exist if the regex matched")
                    .as_str(),
            );
            path.into()
        })
        .chain(vec![interpreter.into()])
        .collect())
}

#[tracing::instrument(err, ret)]
fn copy_dep(dep: &Path, dst: &Path) -> Result<()> {
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
    if dst.exists() {
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
        copy_with_metadata(&dep, dst, None, None)?;
    }
    Ok(())
}
