/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::collections::HashSet;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use antlir2_path::PathExt;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use anyhow::bail;
use anyhow::ensure;
use cap_std::fs::Dir;
use clap::Parser;
use nix::unistd::Gid;
use nix::unistd::Uid;
use walkdir::WalkDir;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    subvol_symlink: PathBuf,
    #[clap(long)]
    rootless: bool,
    #[clap(long)]
    path: PathBuf,
    #[clap(long)]
    out: PathBuf,
}

fn main() -> Result<()> {
    let args = Args::parse();
    tracing_subscriber::fmt()
        .with_writer(std::io::stderr)
        .with_max_level(tracing::Level::TRACE)
        .init();

    let rootless = match args.rootless {
        true => None,
        false => Some(antlir2_rootless::init().context("while setting up antlir2_rootless")?),
    };
    if args.rootless {
        antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
    }

    let root_guard = rootless.map(|r| r.escalate()).transpose()?;
    antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;

    let mut copy_files = HashMap::new();
    let mut create_dirs = HashSet::new();
    let mut create_symlinks = HashMap::new();

    // open all the files that need to be cloned as the (maybe) privileged image user

    let src = args.subvol_symlink.join_abs(&args.path);
    let meta = std::fs::metadata(&src)?;
    if meta.is_dir() {
        create_dirs.insert(args.out.clone());
        for entry in WalkDir::new(&src) {
            let entry = entry?;
            let path = entry.path();
            let relpath = path
                .strip_prefix(&src)
                .context("direntry must be under hoist src")?;
            let dst = args.out.join(relpath);
            if entry.file_type().is_dir() {
                create_dirs.insert(dst.clone());
            } else if entry.file_type().is_symlink() {
                let target = std::fs::read_link(path)?;
                create_symlinks.insert(dst.clone(), target);
            } else if entry.file_type().is_file() {
                let src_file = File::open(path)
                    .with_context(|| format!("while opening src file {}", path.display()))?;
                copy_files.insert(dst.clone(), (src_file, path.to_owned()));
            } else {
                bail!("not a dir, symlink or file - hoist can't do anything with it");
            }
        }
    } else {
        let src_file = File::open(&src)
            .with_context(|| format!("while opening src file {}", src.display()))?;
        copy_files.insert(args.out.clone(), (src_file, src));
    }

    // then switch back to the unprivileged "buck2 build" user before actually
    // creating any outputs
    drop(root_guard);

    for path in create_dirs {
        std::fs::create_dir_all(&path)
            .with_context(|| format!("while creating output dir {}", path.display()))?;
    }

    for (dst_path, (mut src_file, src_path)) in copy_files {
        let mut dst = File::create(&dst_path)?;
        let src_meta = src_file.metadata()?;
        ensure!(
            !src_meta.is_symlink(),
            "cannot hoist symlink {}",
            src_path.display()
        );
        std::io::copy(&mut src_file, &mut dst)?;
        std::fs::set_permissions(&dst_path, src_meta.permissions())?;
    }
    for (dst_path, target) in create_symlinks {
        std::os::unix::fs::symlink(target, &dst_path)?;
    }

    Ok(())
}
