/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeSet;
use std::collections::HashSet;
use std::ffi::OsStr;
use std::fs::File;
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
use extract::ensure_usr;
use extract::so_dependencies;
use nix::unistd::Uid;
use tracing::debug;

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
}

fn write_manifest(entries: BTreeSet<ManifestEntry>, path: &Path) -> Result<()> {
    let f = BufWriter::new(File::create(path)?);
    serde_json::to_writer_pretty(f, &Manifest(entries))?;
    Ok(())
}

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
    let default_interpreter = extract::default_interpreter(args.target_arch);
    let src = args.src.canonicalize()?;
    let deps = so_dependencies(args.src.to_owned(), None, default_interpreter)?;

    let mut entries = BTreeSet::new();

    let main_relpath = PathBuf::from("__main");
    std::fs::create_dir_all(&args.libs_dir)?;
    std::fs::copy(&src, args.libs_dir.join(&main_relpath))?;
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
        std::fs::copy(dep, copy_path)?;

        entries.insert(ManifestEntry::File { src_relpath, dst });
    }

    write_manifest(entries, &args.manifest)
}

fn from_layer(args: FromLayerArgs) -> Result<()> {
    // Set up namespace isolation so that so_dependencies can use unshare
    // to run ld.so --list inside the layer's sysroot.
    antlir2_rootless::init().context("while setting up antlir2_rootless")?;
    if !Uid::effective().is_root() {
        antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
    }
    antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;

    let default_interpreter = extract::default_interpreter(args.target_arch);
    let src_layer = args
        .layer
        .canonicalize()
        .context("while looking up abspath of src layer")?;

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
            so_dependencies(real_binary_in_layer, Some(&src_layer), default_interpreter)?
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
