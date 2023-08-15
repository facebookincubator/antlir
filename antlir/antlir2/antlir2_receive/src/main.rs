/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;
use std::process::Command;
use std::time::SystemTime;

use antlir2_btrfs::DeleteFlags;
use antlir2_btrfs::Subvolume;
use antlir2_working_volume::WorkingVolume;
use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use buck_label::Label;
use clap::Parser;
use clap::ValueEnum;
use tracing::trace;
use tracing_subscriber::prelude::*;

#[derive(Parser, Debug)]
/// Receive a pre-built image package into the local working volume.
pub(crate) struct Receive {
    #[clap(long)]
    /// Label of the image being build
    label: Label,
    #[clap(long)]
    /// Path to the image file
    source: PathBuf,
    #[clap(long, value_enum)]
    /// Format of the image file
    format: Format,
    #[clap(long)]
    /// buck-out path to store the reference to this volume
    output: PathBuf,
    #[clap(flatten)]
    setup: SetupArgs,
}

#[derive(Debug, Copy, Clone, ValueEnum)]
pub enum Format {
    #[clap(name = "sendstream")]
    Sendstream,
}

#[derive(Parser, Debug)]
struct SetupArgs {
    #[clap(long)]
    /// Path to the working volume where images should be built
    working_dir: PathBuf,
}

impl Receive {
    /// Make sure that the working directory exists and clean up any existing
    /// version of the subvolume that we're receiving.
    #[tracing::instrument(skip(self), ret, err)]
    fn prepare_dst(&self) -> Result<PathBuf> {
        let working_volume = WorkingVolume::ensure(self.setup.working_dir.clone())
            .context("while setting up WorkingVolume")?;
        if self.output.exists() {
            let subvol = Subvolume::open(&self.output).context("while opening existing subvol")?;
            subvol
                .delete(DeleteFlags::RECURSIVE)
                .map_err(|(_subvol, err)| err)
                .with_context(|| {
                    format!("while deleting existing subvol {}", self.output.display())
                })?;
            std::fs::remove_file(&self.output).context("while deleting existing symlink")?;
        }
        // Encode the current time into the subvol name so that the symlink's
        // cache key changes if the underlying image changes, otherwise it will
        // point to the same path, so downstream artifacts will not get rebuilt
        // since it appears to be identical, even though the thing behind the
        // symlink has been changed.
        let dst = working_volume.join(format!(
            "{}-{}-received",
            SystemTime::now()
                .duration_since(SystemTime::UNIX_EPOCH)
                .expect("time travelers shouldn't be building images")
                .as_secs(),
            self.label.flat_filename(),
        ));
        Ok(dst)
    }

    #[tracing::instrument(name = "receive", skip(self))]
    pub(crate) fn run(self) -> Result<()> {
        let dst = self.prepare_dst()?;

        let recv_tmp = tempfile::tempdir_in(&self.setup.working_dir)?;
        // If there are more [Format]s here, we'll need to match on it to do
        // different things
        let mut cmd = Command::new("btrfs");
        cmd.arg("--quiet")
            .arg("receive")
            .arg(recv_tmp.path())
            .arg("-f")
            .arg(&self.source);
        trace!("receiving sendstream: {cmd:?}");
        let res = cmd.spawn()?.wait()?;
        ensure!(res.success(), "btrfs-receive failed");
        let entries: Vec<_> = std::fs::read_dir(&recv_tmp)
            .context("while reading tmp dir")?
            .map(|r| {
                r.map(|entry| entry.path())
                    .context("while iterating tmp dir")
            })
            .collect::<Result<_>>()?;
        if entries.len() != 1 {
            return Err(anyhow!(
                "did not get exactly one subvolume received: {entries:?}"
            ));
        }

        trace!("opening received subvol: {}", entries[0].display());
        let mut subvol = Subvolume::open(&entries[0]).context("while opening subvol")?;
        subvol
            .set_readonly(false)
            .context("while making subvol rw")?;

        trace!(
            "moving received subvol to right location {} -> {}",
            subvol.path().display(),
            dst.display()
        );
        std::fs::rename(subvol.path(), &dst).context("while renaming subvol")?;

        let mut subvol = Subvolume::open(&dst).context("while opening subvol")?;
        subvol
            .set_readonly(true)
            .context("while making subvol ro")?;

        let _ = std::fs::remove_file(&self.output);
        std::os::unix::fs::symlink(subvol.path(), &self.output).context("while making symlink")?;
        Ok(())
    }
}

fn main() -> Result<()> {
    let args = Receive::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::Layer::default()
                .event_format(
                    tracing_glog::Glog::default()
                        .with_span_context(true)
                        .with_timer(tracing_glog::LocalTime::default()),
                )
                .fmt_fields(tracing_glog::GlogFields::default()),
        )
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    args.run()
}
