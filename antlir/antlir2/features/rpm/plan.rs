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
use anyhow::Context;
use anyhow::Result;
use buck_label::Label;
use cad_stack_fs::extract_root_dir;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use clap::Parser;
use json_arg::Json;
use json_arg::JsonFile;
use rpm::DriverContext;
use rpm::RpmItem;
use tempfile::TempDir;

#[derive(Debug, Parser)]
struct Args {
    #[clap(long)]
    rootless: bool,
    #[clap(long)]
    label: Label,
    #[clap(long)]
    build_appliance: PathBuf,
    #[clap(long, conflicts_with = "parent_cad_stack")]
    parent_subvol_symlink: Option<PathBuf>,
    #[clap(long)]
    parent_cad_stack: Vec<PathBuf>,
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
    resolve_cmd: Vec<String>,
    #[clap(long)]
    versionlock_hard_enforce: bool,
    #[clap(long)]
    out: PathBuf,
}

enum Parent {
    Subvol(PathBuf),
    CadStack(TempDir),
    None,
}

impl Parent {
    fn open(subvol: Option<PathBuf>, layers: &[PathBuf]) -> Result<Self> {
        if let Some(subvol) = subvol {
            return Ok(Self::Subvol(subvol));
        }
        let Some(top) = layers.first() else {
            return Ok(Self::None);
        };
        let tmpdir = TempDir::new().context("while creating parent materialization tmpdir")?;
        let store = cad_stack::ObjectStore::open_rw(
            top.join("store"),
            layers.iter().skip(1).map(|p| p.join("store")),
        )
        .context("while opening parent cad-stack store")?;
        extract_root_dir(
            &store,
            &Dir::open_ambient_dir(tmpdir.path(), ambient_authority())
                .context("while opening parent root")?,
        )
        .context("while materializing parent cad-stack")?;
        Ok(Self::CadStack(tmpdir))
    }

    fn path(&self) -> Option<&Path> {
        match self {
            Self::Subvol(p) => Some(p),
            Self::CadStack(tmpdir) => Some(tmpdir.path()),
            Self::None => None,
        }
    }
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let args = Args::parse();

    // When running with sudo (not rootless), drop privileges so that the
    // output file is created with the correct ownership, preventing
    // root-owned files from being left in buck-out.
    let rootless_ctx = if !args.rootless {
        Some(antlir2_rootless::init().context("while setting up antlir2_rootless")?)
    } else {
        None
    };

    // Create the output file as the unprivileged user before entering any
    // privileged section. Even if the process crashes, this ensures no
    // root-owned files are left in buck-out.
    File::create(&args.out).context("while creating output file")?;

    if args.rootless {
        antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
    }

    // Re-escalate to root for mount namespace isolation and planning
    let _root_guard = rootless_ctx
        .map(|r| r.escalate())
        .transpose()
        .context("while re-escalating privileges")?;

    antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;

    let parent = Parent::open(args.parent_subvol_symlink, &args.parent_cad_stack)?;

    let rpm = rpm::Rpm {
        items: args.items.into_inner(),
        driver_cmd: args.resolve_cmd,
        internal_only_options: Default::default(),
        versionlock_hard_enforce: args.versionlock_hard_enforce,
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
    let out = BufWriter::new(File::create(&args.out).context("while opening output file")?);
    serde_json::to_writer(out, &tx).context("while serializing plan")?;
    Ok(())
}
