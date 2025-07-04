/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;

use antlir2_btrfs::Subvolume;
use antlir2_working_volume::WorkingVolume;
use anyhow::Context;
use clap::Parser;
use clap::ValueEnum;
use tracing::trace;
use tracing::warn;
use tracing_subscriber::prelude::*;

#[cfg(facebook)]
mod facebook;
mod sendstream;

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
    #[clap(long)]
    /// Use an unprivileged usernamespace
    rootless: bool,
    #[clap(long, default_value = "btrfs")]
    /// path to 'btrfs' command
    btrfs: PathBuf,
    #[clap(long)]
    facts_db_out: PathBuf,
    #[clap(long)]
    build_appliance: Option<PathBuf>,
}

#[derive(Debug, Copy, Clone, ValueEnum)]
pub enum Format {
    Sendstream,
    Tar,
    #[cfg(facebook)]
    Caf,
}

#[derive(Debug, thiserror::Error)]
enum Error {
    #[error("failed to setup working volume: {0}")]
    WorkingVolume(#[from] antlir2_working_volume::Error),
    #[error(transparent)]
    Btrfs(#[from] antlir2_btrfs::Error),
    #[error(transparent)]
    Rootless(#[from] antlir2_rootless::Error),
    #[error("{0:#?}")]
    IO(#[from] std::io::Error),
    #[error("{0:#?}")]
    Uncategorized(#[from] anyhow::Error),
}

type Result<T> = std::result::Result<T, Error>;

impl Error {
    fn category(&self) -> Option<&'static str> {
        match self {
            Error::WorkingVolume(_) => Some("working_volume"),
            Error::Btrfs(_) => Some("btrfs"),
            Error::Rootless(_) => Some("rootless"),
            _ => None,
        }
    }
}

impl Receive {
    /// Make sure that the working directory exists and clean up any existing
    /// version of the subvolume that we're receiving.
    #[tracing::instrument(skip(self), ret, err(Debug))]
    fn prepare_dst(&self, working_volume: &WorkingVolume) -> Result<PathBuf> {
        let dst = working_volume.allocate_new_subvol_path()?;
        trace!("WorkingVolume gave us new path {}", dst.display());

        Ok(dst)
    }

    #[tracing::instrument(name = "receive", skip(self))]
    pub(crate) fn run(self) -> Result<()> {
        let rootless = if !self.rootless {
            Some(antlir2_rootless::init().context("while setting up antlir2_rootless")?)
        } else {
            None
        };

        // setting up the WorkingVolume *must* happen before entering a new
        // mount namespace, otherwise we won't actually see the eden redirection
        trace!("setting up WorkingVolume");
        let working_volume = WorkingVolume::ensure()?;

        if self.rootless {
            antlir2_rootless::unshare_new_userns().context("while setting up userns")?;
            antlir2_isolate::unshare_and_privatize_mount_ns()
                .context("while isolating mount ns")?;
        };

        let dst = self.prepare_dst(&working_volume)?;

        let root = rootless.map(|r| r.escalate()).transpose()?;

        match self.format {
            Format::Sendstream => {
                sendstream::recv_sendstream(&self, &dst, &working_volume)?;
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
                facebook::caf::recv_caf(&self.source, &dst).context("while receiving caf")?;
            }
        };
        let mut subvol = Subvolume::open(&dst).context("while opening subvol")?;

        subvol
            .set_readonly(true)
            .context("while making subvol ro")?;

        antlir2_facts::update_db::sync_db_with_layer()
            .db(&self.facts_db_out)
            .layer(subvol.path())
            .maybe_build_appliance(self.build_appliance.as_deref())
            .call()
            .context("while updating facts db with layer contents")?;

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

    let res = args.run();
    if let Err(e) = &res {
        if let Some(category) = e.category() {
            antlir2_error_handler::SubError::builder()
                .category(category)
                .message(e)
                .build()
                .log();
        }
    }
    res
}
