/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::process::Command;

use antlir2_btrfs::Subvolume;
use antlir2_cas_dir::CasDir;
use antlir2_working_volume::WorkingVolume;
use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use clap::ValueEnum;
use tracing::trace;
use tracing::warn;
use tracing_subscriber::prelude::*;

#[cfg(facebook)]
mod caf;

#[derive(Parser, Debug)]
/// Receive a pre-built image package into the local working volume.
pub(crate) struct Receive {
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
    #[clap(long)]
    /// Use an unprivileged usernamespace
    rootless: bool,
    #[clap(long, default_value = "btrfs")]
    /// path to 'btrfs' command
    btrfs: PathBuf,
}

#[derive(Debug, Copy, Clone, ValueEnum)]
pub enum Format {
    Sendstream,
    #[clap(name = "cas_dir")]
    CasDir,
    Tar,
    #[cfg(facebook)]
    Caf,
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
    #[tracing::instrument(skip(self), ret, err(Debug))]
    fn prepare_dst(&self, working_volume: &WorkingVolume) -> Result<PathBuf> {
        let dst = working_volume
            .allocate_new_path()
            .context("while allocating new path for subvol")?;
        trace!("WorkingVolume gave us new path {}", dst.display());

        Ok(dst)
    }

    #[tracing::instrument(name = "receive", skip(self))]
    pub(crate) fn run(self) -> Result<()> {
        trace!("setting up WorkingVolume");
        let working_volume = WorkingVolume::ensure(self.setup.working_dir.clone())
            .context("while setting up WorkingVolume")?;

        let rootless = if self.rootless {
            antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
            antlir2_isolate::unshare_and_privatize_mount_ns()
                .context("while isolating mount ns")?;
            None
        } else {
            Some(antlir2_rootless::init().context("while setting up antlir2_rootless")?)
        };

        let dst = self.prepare_dst(&working_volume)?;

        let root = rootless.map(|r| r.escalate()).transpose()?;

        match self.format {
            Format::Sendstream => {
                let recv_tmp = tempfile::tempdir_in(&self.setup.working_dir)?;
                let mut cmd = Command::new(self.btrfs);
                cmd.arg("--quiet")
                    .arg("receive")
                    .arg(recv_tmp.path())
                    .arg("-f")
                    .arg(&self.source);
                if self.rootless {
                    cmd.arg("--force-decompress");
                }
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
            }
            Format::CasDir => {
                let subvol = Subvolume::create(&dst).context("while creating subvol")?;
                let cas_dir = CasDir::open(self.source).context("while opening CasDir")?;
                cas_dir
                    .hydrate_into(subvol.path())
                    .context("while materializing CasDir")?;
            }
            Format::Tar => {
                let subvol = Subvolume::create(&dst).context("while creating subvol")?;
                let mut archive =
                    tar::Archive::new(BufReader::new(File::open(&self.source).with_context(
                        || format!("while opening source file {}", self.source.display()),
                    )?));
                archive
                    .unpack(subvol.path())
                    .context("while unpacking tar")?;
            }
            #[cfg(facebook)]
            Format::Caf => {
                caf::recv_caf(&self.source, &dst).context("while receiving caf")?;
            }
        };
        let mut subvol = Subvolume::open(&dst).context("while opening subvol")?;

        subvol
            .set_readonly(true)
            .context("while making subvol ro")?;

        if self.output.exists() {
            trace!("removing existing output {}", self.output.display());
            // Don't fail if the old subvol couldn't be deleted, just print
            // a warning. We really don't want to fail someone's build if
            // the only thing that went wrong is not being able to delete
            // the last version of it.
            match Subvolume::open(&self.output) {
                Ok(old_subvol) => {
                    if let Err(e) = old_subvol.delete() {
                        warn!(
                            "couldn't delete old subvol '{}': {e:?}",
                            self.output.display()
                        );
                    }
                }
                Err(e) => {
                    warn!(
                        "couldn't open old subvol '{}': {e:?}",
                        self.output.display()
                    );
                }
            }
        }
        drop(root);

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
