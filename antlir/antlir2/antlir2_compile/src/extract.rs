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
use std::os::unix::process::CommandExt;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_features::extract::Extract;
use anyhow::Context;
use goblin::elf::Elf;
use once_cell::sync::Lazy;
use regex::Regex;
use twox_hash::XxHash64;

use crate::util::copy_with_metadata;
use crate::CompileFeature;
use crate::CompilerContext;
use crate::Error;
use crate::Result;

/// Simple regex to parse the output of `ld.so --list` which is used to resolve
/// the dependencies of a binary.
static LDSO_RE: Lazy<Regex> = Lazy::new(|| {
    regex::RegexBuilder::new(r#"^\s*(?P<name>.+)\s+=>\s+(?P<path>.+)\s+\(0x[0-9a-f]+\)$"#)
        .multi_line(true)
        .build()
        .expect("this is a valid regex")
});

// Using the target architecture here is fine, because we'll be executing the
// target-arch version of antlir2 inside of an arch-specific ba container when
// doing cross-arch image builds.
#[cfg(target_arch = "x86_64")]
static DEFAULT_LD_SO: &str = "/usr/lib64/ld-linux-x86-64.so.2";

#[cfg(target_arch = "aarch64")]
static DEFAULT_LD_SO: &str = "/lib/ld-linux-aarch64.so.1";

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
fn so_dependencies<S: AsRef<OsStr>>(
    binary: S,
    sysroot: Option<&Path>,
) -> anyhow::Result<Vec<PathBuf>> {
    let binary = Path::new(binary.as_ref());
    let binary_as_seen_from_here = match sysroot {
        Some(sysroot) => {
            Cow::Owned(sysroot.join(Path::new(binary).strip_prefix("/").unwrap_or(binary)))
        }
        None => Cow::Borrowed(binary),
    };
    let buf = std::fs::read(&binary_as_seen_from_here)
        .with_context(|| format!("while reading {}", binary_as_seen_from_here.display()))?;
    let elf =
        Elf::parse(&buf).with_context(|| format!("while parsing ELF {}", binary.display()))?;
    let interpreter = Path::new(elf.interpreter.unwrap_or(DEFAULT_LD_SO));
    tracing::debug!("using interpreter {}", interpreter.display());
    let mut cmd = Command::new(interpreter);
    if let Some(sysroot) = sysroot {
        let sysroot = sysroot.to_path_buf();
        unsafe {
            cmd.pre_exec(move || {
                std::os::unix::fs::chroot(&sysroot)?;
                std::env::set_current_dir("/")?;
                Ok(())
            });
        }
    }

    let output = cmd
        .arg("--list")
        .arg(binary)
        // There's a memory allocation bug under qemu-aarch64 when asking the linker to --list
        // an elf binary.  This configures qemu-aarch64 to pre-allocate enough virtual address
        // space to not exploded in this case.  This env var has no effect when running on the
        // native host (x86_64 or aarch64).
        // TODO: Remove this after the issue is found and fixed with qemu-aarch64.
        .env("QEMU_RESERVED_VA", "0x40000000")
        .output()
        .with_context(|| format!("while listing libraries for {:?}", binary))?;
    anyhow::ensure!(
        output.status.success(),
        "{} failed with exit code {}: {}\n{}",
        interpreter.display(),
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
                std::fs::create_dir(dir)?;
            }
        }
    }
    let metadata = std::fs::symlink_metadata(dep)
        .with_context(|| format!("while statting '{}'", dep.display()))?;
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

        if pre_existing_hash != new_src_hash {
            return Err(Error::ExtractConflict(dst.to_path_buf()));
        }
    } else {
        copy_with_metadata(&dep, dst, None, None)?;
    }
    Ok(())
}

impl<'a> CompileFeature for Extract<'a> {
    #[tracing::instrument(name = "extract", skip(ctx), ret, err)]
    fn compile(&self, ctx: &CompilerContext) -> Result<()> {
        match self {
            Self::Buck(buck) => {
                let src = buck.src.path().canonicalize()?;
                let deps = so_dependencies(buck.src.path(), None)?;
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
                            ),
                        )?;
                    } else {
                        copy_dep(dep, &ctx.dst_path(dep.strip_prefix("/").unwrap_or(dep)))?;
                    }
                }
                // don't copy the metadata from the buck binary, the owner will
                // be wrong
                std::fs::copy(buck.src.path(), ctx.dst_path(&buck.dst))?;
                Ok(())
            }
            Self::Layer(layer) => {
                let src_layer = layer
                    .layer
                    .subvol_symlink
                    .canonicalize()
                    .context("while looking up abspath of src layer")?;
                tracing::trace!("extract root = {}", src_layer.display());
                let mut all_deps = HashSet::new();
                for binary in &layer.binaries {
                    let dst = ctx.dst_path(binary.path());
                    all_deps.extend(
                        so_dependencies(binary.path(), Some(&src_layer))?
                            .into_iter()
                            .map(|path| ensure_usr(&path).to_path_buf()),
                    );
                    let src =
                        src_layer.join(binary.path().strip_prefix("/").unwrap_or(binary.path()));
                    copy_with_metadata(&src, &dst, None, None)?;
                }
                for dep in all_deps {
                    copy_dep(
                        &src_layer.join(dep.strip_prefix("/").unwrap_or(&dep)),
                        &ctx.dst_path(&dep),
                    )?;
                }
                Ok(())
            }
        }
    }
}
