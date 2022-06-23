/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

use std::future::Future;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context, Result};

use clap::Parser;
use slog::Logger;

use fbthrift::simplejson_protocol::Serializable;
use metalos_host_configs::api::OfflineUpdateRequest;
use state::State;

mod offline;
mod online;

// For now anyway, the interface for online and offline updates are exactly the
// same, even though the implementation is obviously different.

#[derive(Parser)]
pub(crate) enum Subcommand {
    /// Download images and do some preflight checks
    Stage(CommonOpts),
    /// Apply the new config
    Commit(CommonOpts),
}

impl Subcommand {
    pub(self) fn load_input<S, Ser>(&self) -> Result<S>
    where
        S: State<Ser>,
        Ser: state::Serialization,
    {
        match self {
            Self::Stage(c) | Self::Commit(c) => c.load(),
        }
    }
}

#[derive(Parser)]
pub(crate) enum Update {
    #[clap(subcommand, name = "offline-update")]
    /// Update boot config (with host downtime)
    Offline(Subcommand),
    #[clap(subcommand, name = "online-update")]
    /// Update runtime config (without host downtime)
    Online(Subcommand),
}

#[derive(Parser)]
pub(crate) struct CommonOpts {
    json_path: PathBuf,
}

impl CommonOpts {
    pub(self) fn load<S, Ser>(&self) -> Result<S>
    where
        S: State<Ser>,
        Ser: state::Serialization,
    {
        let input = if self.json_path == Path::new("-") {
            let mut input = Vec::new();
            std::io::stdin()
                .read_to_end(&mut input)
                .context("while reading stdin")?;
            input
        } else {
            std::fs::read(&self.json_path)
                .with_context(|| format!("while reading {}", self.json_path.display()))?
        };
        S::from_json(input).context("while deserializing")
    }
}

async fn run_subcommand<F, Fut, Input, Return, Error>(
    func: F,
    log: Logger,
    input: Input,
) -> anyhow::Result<()>
where
    Return: Serializable,
    Error: std::fmt::Debug + Serializable,
    F: Fn(Logger, Input) -> Fut,
    Fut: Future<Output = std::result::Result<Return, Error>>,
{
    match func(log, input).await {
        Ok(resp) => {
            let output = fbthrift::simplejson_protocol::serialize(&resp);
            std::io::stdout()
                .write_all(&output)
                .context("while writing response")?;
            println!();
            Ok(())
        }
        Err(err) => {
            let output = fbthrift::simplejson_protocol::serialize(&err);
            std::io::stdout()
                .write_all(&output)
                .with_context(|| format!("while writing error {:?}", err))?;
            println!();
            Err(anyhow!("{:?}", err))
        }
    }
}

impl Update {
    pub(crate) async fn subcommand(self, log: Logger) -> Result<()> {
        match self {
            Self::Offline(sub) => {
                let req: OfflineUpdateRequest = sub.load_input()?;
                match sub {
                    Subcommand::Stage(_) => {
                        run_subcommand(offline::stage, log, req.boot_config).await
                    }
                    Subcommand::Commit(_) => {
                        run_subcommand(offline::commit, log, req.boot_config).await
                    }
                }
            }
            Self::Online(sub) => {
                let runtime_config = sub.load_input()?;
                match sub {
                    Subcommand::Stage(_) => {
                        run_subcommand(online::stage, log, runtime_config).await
                    }
                    Subcommand::Commit(_) => {
                        run_subcommand(online::commit, log, runtime_config).await
                    }
                }
            }
        }
    }
}
