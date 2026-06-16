/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use antlir2_working_volume::WorkingFormat;
use antlir2_working_volume::WorkingVolume;
use anyhow::Context;
use anyhow::Result;
use anyhow::anyhow;
use cad_stack_fs::extract_root_dir;
use cap_std::ambient_authority;
use cap_std::fs::Dir;
use clap::Parser;
use json_arg::JsonFile;

mod btrfs;
mod cad_stack;
mod cpio;
mod docker_archive;
mod erofs;
mod ext3;
mod ext4;
mod gpt;
mod oci;
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
    layer: Vec<PathBuf>,
    #[clap(long)]
    working_format: WorkingFormat,
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

pub(crate) fn pad_to_align(path: &Path, align: u64) -> Result<()> {
    use std::fs::OpenOptions;
    let len = std::fs::metadata(path)
        .with_context(|| format!("while getting size of {}", path.display()))?
        .len();
    let new = if align == 0 {
        len
    } else {
        len.checked_next_multiple_of(align).with_context(|| {
            format!(
                "align_up overflow: cannot align {} to boundary {} while aligning {}",
                len,
                align,
                path.display()
            )
        })?
    };
    if new <= len {
        return Ok(());
    }
    let f = OpenOptions::new()
        .write(true)
        .open(path)
        .with_context(|| format!("while opening {} for padding", path.display()))?;
    // set_len creates a sparse hole on supporting filesystems, otherwise the file is extended with 0s
    f.set_len(new)
        .with_context(|| format!("while padding {} to {}", path.display(), new))?;
    Ok(())
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

    let materialize_layer = || match args.working_format {
        WorkingFormat::Btrfs => {
            if args.layer.len() != 1 {
                Err(anyhow!("btrfs requires exactly one layer"))
            } else {
                Ok(args.layer[0].clone())
            }
        }
        WorkingFormat::CadStack => {
            let wv = WorkingVolume::ensure()?;
            let path = wv.allocate_new_subvol_path()?;
            // Try to create a btrfs subvolume so that formats like sendstream
            // (which need a real subvolume) work correctly. Fall back to a
            // plain directory for environments without btrfs (e.g. RE).
            if antlir2_btrfs::Subvolume::create(&path).is_err() {
                std::fs::create_dir_all(&path)?;
            }

            let store = ::cad_stack::ObjectStore::open_rw(
                args.layer[0].join("store"),
                args.layer.iter().skip(1).map(|p| p.join("store")),
            )
            .context("while opening root store")?;
            extract_root_dir(
                &store,
                &Dir::open_ambient_dir(&path, ambient_authority())
                    .context("while opening new root")?,
            )
            .context("while re-hydrating cad-stack")?;
            Ok(path)
        }
    };

    match args.spec.into_inner() {
        Spec::Btrfs(p) => p.build(&args.out),
        Spec::CadStack(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
        ),
        Spec::Cpio(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
        ),
        Spec::DockerArchive(p) => p.build(&args.out),
        Spec::Erofs(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
        ),
        Spec::Ext3(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
        ),
        Spec::Ext4(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
        ),
        Spec::Gpt(p) => p.build(&args.out),
        Spec::Oci(p) => p.build(&args.out),
        Spec::Rpm(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
        ),
        Spec::Sendstream(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
        ),
        Spec::Squashfs(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
        ),
        Spec::Tar(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
        ),
        Spec::UnprivilegedDir(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
            root_guard,
        ),
        Spec::Vfat(p) => p.build(
            &args.out,
            &materialize_layer().context("layer required for this format")?,
        ),
        Spec::Xar(p) => p.build(&args.out),
    }
}
