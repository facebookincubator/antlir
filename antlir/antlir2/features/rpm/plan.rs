/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use std::path::PathBuf;

use antlir2_compile::Arch;
use antlir2_overlayfs::OverlayFs;
use anyhow::Context;
use anyhow::Result;
use buck_label::Label;
use clap::Parser;
use json_arg::Json;
use json_arg::JsonFile;
use nix::mount::mount;
use nix::mount::MsFlags;
use nix::sched::unshare;
use nix::sched::CloneFlags;
use rpm::DriverContext;
use rpm::RpmItem;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    rootless: bool,
    #[clap(long)]
    label: Label,
    #[clap(long)]
    build_appliance: PathBuf,
    #[clap(long, conflicts_with = "parent_overlayfs")]
    parent_subvol_symlink: Option<PathBuf>,
    #[clap(long, conflicts_with = "parent_subvol_symlink")]
    parent_overlayfs: Option<JsonFile<antlir2_overlayfs::BuckModel>>,
    #[clap(long)]
    repodatas: PathBuf,
    #[clap(long)]
    versionlock: Option<JsonFile<HashMap<String, String>>>,
    #[clap(long)]
    versionlock_extend: Json<HashMap<String, String>>,
    #[clap(long)]
    exclude_rpm: Vec<String>,
    #[clap(long)]
    target_arch: Arch,
    #[clap(long)]
    items: JsonFile<Vec<RpmItem>>,
    #[clap(long)]
    out: PathBuf,
}

enum Parent {
    Subvol(PathBuf),
    Overlayfs(OverlayFs),
    None,
}

impl Parent {
    fn path(&self) -> Option<&Path> {
        match self {
            Self::Subvol(p) => Some(p),
            Self::Overlayfs(o) => Some(o.mountpoint()),
            Self::None => None,
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let args = Args::parse();

    if args.rootless {
        antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
    }

    // Be careful to isolate this process from the host mount namespace in
    // case anything weird is going on
    unshare(CloneFlags::CLONE_NEWNS).context("while unsharing mount")?;

    // Remount / as private so that we don't let any changes escape back
    // to the parent mount namespace (basically equivalent to `mount
    // --make-rprivate /`)
    mount(
        None::<&str>,
        "/",
        None::<&str>,
        MsFlags::MS_REC | MsFlags::MS_PRIVATE,
        None::<&str>,
    )
    .context("while making mount ns private")?;

    let parent = match (args.parent_subvol_symlink, args.parent_overlayfs) {
        (Some(parent_subvol), None) => Parent::Subvol(parent_subvol),
        (None, Some(model)) => {
            let fs = OverlayFs::mount(
                antlir2_overlayfs::Opts::builder()
                    .model(model.into_inner())
                    .build(),
            )
            .context("while mounting overlayfs")?;
            Parent::Overlayfs(fs)
        }
        (None, None) => Parent::None,
        _ => unreachable!("impossible combination of args"),
    };

    let rpm = rpm::Rpm {
        items: args.items.into_inner(),
        ..Default::default()
    };
    let tx = rpm
        .plan(DriverContext::plan(
            args.label,
            parent.path().map(Path::to_owned),
            args.build_appliance,
            args.repodatas,
            args.target_arch,
            args.versionlock
                .map(JsonFile::into_inner)
                .unwrap_or_default()
                .into_iter()
                .chain(args.versionlock_extend.into_inner())
                .collect(),
            args.exclude_rpm.into_iter().collect(),
        ))
        .context("while planning transaction")?;
    let out = BufWriter::new(File::create(&args.out).context("while creating output file")?);
    serde_json::to_writer(out, &tx).context("while serializing plan")?;
    Ok(())
}
