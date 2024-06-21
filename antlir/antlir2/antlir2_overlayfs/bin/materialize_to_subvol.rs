/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::DirBuilder;
use std::os::unix::fs::DirBuilderExt;
use std::os::unix::fs::MetadataExt;
use std::path::Path;
use std::path::PathBuf;

use antlir2_btrfs::Subvolume;
use antlir2_overlayfs::OverlayFs;
use antlir2_working_volume::WorkingVolume;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;
use tracing::trace;
use tracing::warn;
use walkdir::WalkDir;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    model: JsonFile<antlir2_overlayfs::BuckModel>,
    #[clap(long)]
    subvol_symlink: PathBuf,
    #[clap(long, default_value = "antlir2-out")]
    working_dir: PathBuf,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();
    let args = Args::parse();

    let working_volume = WorkingVolume::ensure(args.working_dir.clone())
        .context("while setting up WorkingVolume")?;

    if args.subvol_symlink.exists() {
        trace!("removing existing output {}", args.subvol_symlink.display());
        // Don't fail if the old subvol couldn't be deleted, just print
        // a warning. We really don't want to fail someone's build if
        // the only thing that went wrong is not being able to delete
        // the last version of it.
        match Subvolume::open(&args.subvol_symlink) {
            Ok(old_subvol) => {
                if let Err(e) = old_subvol.delete() {
                    warn!(
                        "couldn't delete old subvol '{}': {e:?}",
                        args.subvol_symlink.display(),
                    );
                }
            }
            Err(e) => {
                warn!(
                    "couldn't open old subvol '{}': {e:?}",
                    args.subvol_symlink.display(),
                );
            }
        }
    }

    antlir2_rootless::unshare_new_userns().context("while entering new userns")?;
    antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;

    let overlay = OverlayFs::mount(
        antlir2_overlayfs::Opts::builder()
            .model(args.model.into_inner())
            .build(),
    )?;

    let dst_root = working_volume
        .allocate_new_path()
        .context("while allocating new path for subvol")?;

    Subvolume::create(&dst_root).context("while creating new subvol")?;

    for entry in WalkDir::new(overlay.mountpoint()) {
        let entry = entry.context("while walking overlayfs")?;
        let relpath = entry
            .path()
            .strip_prefix(overlay.mountpoint())
            .context("path not below overlayfs")?;
        let dst = if relpath == Path::new(":") {
            dst_root.clone()
        } else {
            dst_root.join(relpath)
        };
        let ft = entry.file_type();
        let meta = entry.metadata()?;
        if ft.is_dir() {
            DirBuilder::new()
                .recursive(true)
                .mode(meta.mode())
                .create(&dst)?;
        } else if ft.is_symlink() {
            let target = entry.path().read_link().context("while reading symlink")?;
            std::os::unix::fs::symlink(target, dst)?;
        } else if ft.is_file() {
            std::fs::copy(entry.path(), &dst)?;
        }
    }

    std::os::unix::fs::symlink(dst_root.canonicalize()?, &args.subvol_symlink)?;

    Ok(())
}
