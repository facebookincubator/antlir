/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::process::Command;

use antlir2_overlayfs::OverlayFs;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;

#[derive(Parser, Debug)]
struct Args {
    #[clap(long)]
    model: JsonFile<antlir2_overlayfs::BuckModel>,
    #[clap(long)]
    bash: String,
}

fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();
    let args = Args::parse();
    antlir2_rootless::unshare_new_userns().context("while entering new userns")?;
    antlir2_isolate::unshare_and_privatize_mount_ns().context("while isolating mount ns")?;

    let overlay = OverlayFs::mount(
        antlir2_overlayfs::Opts::builder()
            .model(args.model.into_inner())
            .build(),
    )?;
    let status = Command::new("bash")
        .arg("-ce")
        .arg(args.bash)
        .current_dir(overlay.mountpoint())
        .spawn()
        .context("while spawning bash")?
        .wait()
        .context("while waiting on bash")?;
    ensure!(status.success(), "populate command failed");
    overlay.finalize().context("while finalizing overlayfs")?;
    Ok(())
}
