/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use json_arg::JsonFile;

mod btrfs;
mod cas_dir;
mod cpio;
mod ext;
mod gpt;
mod rpm;
mod sendstream;
mod spec;
mod squashfs;
mod tar;
mod unprivileged_dir;
mod vfat;
mod xar;
use spec::Spec;
mod build_appliance;
pub(crate) use build_appliance::BuildAppliance;

pub(crate) trait PackageFormat {
    fn build(&self, out: &Path, layer: &Path) -> Result<()>;
}

#[derive(Parser, Debug)]
/// Package an image layer into a file
pub(crate) struct PackageArgs {
    #[clap(long)]
    /// Specifications for the packaging
    spec: JsonFile<Spec>,
    #[clap(long)]
    /// The layer being packaged
    layer: Option<PathBuf>,
    #[clap(long)]
    dir: bool,
    #[clap(long)]
    /// Path to output the image
    out: PathBuf,
    #[clap(long)]
    rootless: bool,
}

pub(crate) fn run_cmd(command: &mut Command) -> Result<std::process::Output> {
    let output = command
        .output()
        .with_context(|| format!("failed to run command: {command:?}"))?;

    match output.status.success() {
        true => Ok(output),
        false => Err(anyhow!(
            "failed to run command {:?} ({:?}): {}\n{}",
            command,
            output.status,
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        )),
    }
}

fn main() -> Result<()> {
    let args = PackageArgs::parse();

    let need_to_reescalate = nix::unistd::Uid::effective().is_root();

    let rootless = antlir2_rootless::init().context("while setting up antlir2_rootless")?;

    if !args.dir {
        std::fs::File::create(&args.out)?;
    }

    if args.rootless {
        antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
    }

    tracing_subscriber::fmt()
        .with_max_level(tracing::Level::TRACE)
        .init();

    let root_guard = if need_to_reescalate {
        Some(rootless.escalate()?)
    } else {
        None
    };

    let layer = args.layer.as_deref();

    match args.spec.into_inner() {
        Spec::Btrfs(p) => p.build(&args.out),
        Spec::CasDir(p) => p.build(&args.out, layer.context("layer required for this format")?),
        Spec::Cpio(p) => p.build(&args.out, layer.context("layer required for this format")?),
        Spec::Ext3(p) => p.build(&args.out, layer.context("layer required for this format")?),
        Spec::Gpt(p) => p.build(&args.out),
        Spec::Rpm(p) => p.build(&args.out, layer.context("layer required for this format")?),
        Spec::Sendstream(p) => p.build(&args.out, layer.context("layer required for this format")?),
        Spec::Squashfs(p) => p.build(&args.out, layer.context("layer required for this format")?),
        Spec::Tar(p) => p.build(&args.out, layer.context("layer required for this format")?),
        Spec::UnprivilegedDir(p) => p.build(
            &args.out,
            layer.context("layer required for this format")?,
            root_guard,
        ),
        Spec::Vfat(p) => p.build(&args.out, layer.context("layer required for this format")?),
        Spec::Xar(p) => p.build(&args.out),
    }
}
