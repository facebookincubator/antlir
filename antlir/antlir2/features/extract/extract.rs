/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::borrow::Cow;
use std::ffi::OsStr;
use std::hash::Hasher;
use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_compile::util::copy_with_metadata;
use antlir2_compile::Arch;
use antlir2_isolate::unshare;
use antlir2_isolate::IsolationContext;
use anyhow::Context;
use anyhow::Result;
use goblin::elf::Elf;
use once_cell::sync::Lazy;
use regex::Regex;
use tracing::trace;
use tracing::warn;
use twox_hash::XxHash64;

/// Simple regex to parse the output of `ld.so --list` which is used to resolve
/// the dependencies of a binary.
static LDSO_RE: Lazy<Regex> = Lazy::new(|| {
    regex::RegexBuilder::new(r"^\s*(?P<name>.+)\s+=>\s+(?P<path>.+)\s+\(0x[0-9a-f]+\)$")
        .multi_line(true)
        .build()
        .expect("this is a valid regex")
});

/// Look up absolute paths to all (recursive) deps of this binary
#[tracing::instrument]
pub fn so_dependencies<S: AsRef<OsStr> + std::fmt::Debug>(
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
        let mut platform = vec![];
        #[cfg(facebook)]
        {
            if sysroot.join("usr/local/fbcode").exists() {
                platform.push(Path::new("/usr/local/fbcode"));
            }
            if sysroot.join("mnt/gvfs").exists() {
                platform.push(Path::new("/mnt/gvfs"));
            }
        }
        cmd = unshare(
            IsolationContext::builder(sysroot)
                .ephemeral(false)
                .platform(platform.as_slice())
                .working_directory(Path::new("/"))
                // There's a memory allocation bug under qemu-aarch64 when
                // asking the linker to --list an elf binary. This configures
                // qemu-aarch64 to pre-allocate enough virtual address space to
                // not explode in this case. This env var has no effect when
                // running on the native host (x86_64 or aarch64).
                // TODO: Remove this after the issue is found and fixed with qemu-aarch64.
                .setenv(("QEMU_RESERVED_VA", "0x40000000"))
                .build(),
        )?
        .command(interpreter)?;
    } else {
        cmd.env("QEMU_RESERVED_VA", "0x40000000");
    }

    cmd.arg("--list").arg(binary);

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
        copy_with_metadata(&dep, dst, None, None)?;
    }
    Ok(())
}

pub fn default_interpreter(target: Arch) -> &'static Path {
    Path::new(match target {
        Arch::X86_64 => "/usr/lib64/ld-linux-x86-64.so.2",
        Arch::Aarch64 => "/lib/ld-linux-aarch64.so.1",
    })
}
