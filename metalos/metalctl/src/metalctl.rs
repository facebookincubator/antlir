/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]

#[cfg(test)]
#[macro_use]
extern crate metalos_macros;

use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::{Error, Result};
use slog::{error, o, trace, Logger};
use structopt::StructOpt;

mod apply_host_config;
mod send_event;
mod stage_host_config;
mod update;

#[derive(StructOpt)]
enum Subcommand {
    /// Download all images specified in the MetalOS host config
    StageHostConfig(stage_host_config::Opts),
    /// Generate and apply a structured host config
    ApplyHostConfig(apply_host_config::Opts),
    /// Send an event to the event endpoint
    SendEvent(send_event::Opts),
    /// Apply a provided disk image to a specified disk and then
    /// upsize it to the maximum size
    #[structopt(flatten)]
    Update(update::Update),
    #[structopt(external_subcommand)]
    External(Vec<String>),
}

#[derive(StructOpt)]
#[structopt(name = "metalctl", no_version)]
struct MetalCtl {
    #[structopt(subcommand)]
    command: Subcommand,
}

async fn run_command(options: MetalCtl, log: Logger) -> Result<()> {
    match options.command {
        Subcommand::StageHostConfig(opts) => stage_host_config::stage_host_config(log, opts).await,
        Subcommand::ApplyHostConfig(opts) => apply_host_config::apply_host_config(log, opts).await,
        Subcommand::SendEvent(opts) => send_event::cmd_send_event(log, opts).await,
        Subcommand::Update(update) => update.subcommand(log).await,
        Subcommand::External(mut args) => {
            let bin = format!("metalctl-{}", args.remove(0));
            trace!(log, "exec-ing external command {}", bin);
            Err(Error::msg(Command::new(bin).args(args).exec()))
        }
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
    match run_command(MetalCtl::from_args(), log.clone()).await {
        Ok(r) => Ok(r),
        Err(e) => {
            error!(log, "{}", e);
            Err(e)
        }
    }
}
