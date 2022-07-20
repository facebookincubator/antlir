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

use anyhow::Error;
use anyhow::Result;
use clap::Parser;
use slog::error;
use slog::o;
use slog::trace;
use slog::Logger;

mod package;
mod send_event;
mod update;

#[derive(Parser)]
enum Subcommand {
    /// Send an event to the event endpoint
    SendEvent(send_event::Opts),
    /// Apply a provided disk image to a specified disk and then
    /// upsize it to the maximum size
    #[clap(flatten)]
    Update(update::Update),
    #[clap(external_subcommand)]
    External(Vec<String>),
    #[clap(subcommand)]
    /// Stage or inspect packages
    Package(package::Opts),
}

#[derive(Parser)]
#[clap(name = "metalctl")]
struct MetalCtl {
    #[clap(subcommand)]
    command: Subcommand,
}

async fn run_command(options: MetalCtl, log: Logger, fb: fbinit::FacebookInit) -> Result<()> {
    match options.command {
        Subcommand::SendEvent(opts) => send_event::cmd_send_event(log, opts).await,
        Subcommand::Update(update) => update.subcommand(log, fb).await,
        Subcommand::External(mut args) => {
            let bin = format!("metalctl-{}", args.remove(0));
            trace!(log, "exec-ing external command {}", bin);
            Err(Error::msg(Command::new(bin).args(args).exec()))
        }
        Subcommand::Package(opts) => match opts {
            package::Opts::Stage(stage) => package::stage_packages(log, fb, stage).await,
            package::Opts::List => package::list(log).await,
        },
    }
}

#[fbinit::main]
async fn main(fb: fbinit::FacebookInit) -> Result<()> {
    let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
    match run_command(MetalCtl::parse(), log.clone(), fb).await {
        Ok(r) => Ok(r),
        Err(e) => {
            error!(log, "{}", e);
            Err(e)
        }
    }
}
