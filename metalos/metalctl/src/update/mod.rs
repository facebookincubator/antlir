/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

use std::fs::File;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;
use slog::Logger;
use structopt::StructOpt;

mod offline;
mod online;

// For now anyway, the interface for online and offline updates are exactly the
// same, even though the implementation is obviously different.

#[derive(StructOpt)]
pub(crate) enum Subcommand {
    /// Download images and do some preflight checks
    Stage(StageOpts),
    /// Apply the new config
    Commit,
}

#[derive(StructOpt)]
pub(crate) enum Update {
    #[structopt(name = "offline-update")]
    /// Update boot config (with host downtime)
    Offline(Subcommand),
    #[structopt(name = "online-update")]
    /// Update runtime config (without host downtime)
    Online(Subcommand),
}

#[derive(StructOpt)]
pub(crate) struct StageOpts {
    json_path: PathBuf,
}

impl StageOpts {
    pub(self) fn load<I>(&self) -> Result<I>
    where
        I: for<'de> Deserialize<'de>,
    {
        if self.json_path == Path::new("-") {
            serde_json::from_reader(std::io::stdin()).context("while deserializing stdin as json")
        } else {
            let f = File::open(&self.json_path)
                .with_context(|| format!("while opening {:?}", &self.json_path))?;
            serde_json::from_reader(f)
                .with_context(|| format!("while deserializing {:?} as json", &self.json_path))
        }
    }
}

impl Update {
    pub(crate) async fn subcommand(self, log: Logger) -> Result<()> {
        match self {
            Self::Online(sub) => online::run(log, sub).await,
            Self::Offline(sub) => offline::run(log, sub).await,
        }
    }
}
