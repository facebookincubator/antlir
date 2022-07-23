/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

use std::future::Future;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;

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
    Commit(CommitOpts),
}

impl Subcommand {
    pub(self) fn load_input<S, Ser>(&self) -> Result<S>
    where
        S: State<Ser>,
        Ser: state::Serialization,
    {
        match self {
            Self::Stage(c) => c.load::<S, Ser>(),
            Self::Commit(c) => c.load::<S, Ser>(),
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

#[derive(Parser)]
#[clap(group = clap::ArgGroup::new("runtime-config").multiple(false).required(true))]
pub(crate) struct CommitOpts {
    #[clap(
        long,
        help = "use last staged config instead of providing the whole struct",
        group = "runtime-config"
    )]
    last_staged: bool,
    #[clap(group = "runtime-config")]
    json_path: Option<PathBuf>,
}

fn load_from_file_arg<S, Ser>(arg: &Path) -> Result<S>
where
    S: State<Ser>,
    Ser: state::Serialization,
{
    let input = if arg == Path::new("-") {
        let mut input = Vec::new();
        std::io::stdin()
            .read_to_end(&mut input)
            .context("while reading stdin")?;
        input
    } else {
        std::fs::read(arg).with_context(|| format!("while reading {}", arg.display()))?
    };
    S::from_json(input).context("while deserializing")
}

impl CommonOpts {
    pub(self) fn load<S, Ser>(&self) -> Result<S>
    where
        S: State<Ser>,
        Ser: state::Serialization,
    {
        load_from_file_arg(&self.json_path)
    }
}

impl CommitOpts {
    pub(self) fn load<S, Ser>(&self) -> Result<S>
    where
        S: State<Ser>,
        Ser: state::Serialization,
    {
        if self.last_staged {
            Ok(S::staged()
                .context("while loading staged config")?
                .context("no staged config")?)
        } else {
            load_from_file_arg(
                self.json_path
                    .as_ref()
                    .context("json_path missing and --last-staged was not specified")?,
            )
        }
    }
}

async fn run_subcommand<F, Fut, Input, Return, Error>(
    func: F,
    log: Logger,
    fb: fbinit::FacebookInit,
    input: Input,
) -> anyhow::Result<()>
where
    Return: Serializable,
    Error: std::fmt::Debug + Serializable,
    F: Fn(Logger, fbinit::FacebookInit, Input) -> Fut,
    Fut: Future<Output = std::result::Result<Return, Error>>,
{
    match func(log, fb, input).await {
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
    pub(crate) async fn subcommand(self, log: Logger, fb: fbinit::FacebookInit) -> Result<()> {
        match self {
            Self::Offline(sub) => {
                let req: OfflineUpdateRequest = sub.load_input()?;
                match sub {
                    Subcommand::Stage(_) => {
                        run_subcommand(offline::stage, log, fb, req.boot_config).await
                    }
                    Subcommand::Commit(_) => {
                        run_subcommand(offline::commit, log, fb, req.boot_config).await
                    }
                }
            }
            Self::Online(sub) => {
                let runtime_config = sub.load_input()?;
                match sub {
                    Subcommand::Stage(_) => {
                        run_subcommand(online::stage, log, fb, runtime_config).await
                    }
                    Subcommand::Commit(_) => {
                        run_subcommand(online::commit, log, fb, runtime_config).await
                    }
                }
            }
        }
    }
}
