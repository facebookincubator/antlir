/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::io::RawFd;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use futures::future::try_join_all;
use plain_systemd::daemon::is_socket;
use plain_systemd::daemon::listen_fds;
use plain_systemd::daemon::Listening;

use fbinit::FacebookInit;

mod thrift_server;
mod update;
use thrift_server::Metald;

#[cfg(facebook)]
mod facebook;

#[derive(Debug, Parser)]
#[clap(name = "Metald Thrift Service")]
struct Arguments {
    #[clap(long, group = "listen")]
    systemd_socket: bool,
    /// Port to serve traffic on
    #[clap(short, long, group = "listen", help = "Listen on TCP port")]
    port: Option<u16>,
}

pub(crate) enum ListenOn {
    Raw(RawFd),
    Port(u16),
}

#[fbinit::main]
async fn main(fb: FacebookInit) -> Result<()> {
    // Process commandline flags
    let args = Arguments::parse();

    let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());

    let metald = Metald::new(fb, log.clone())?;

    let listen_on = match (args.systemd_socket, args.port) {
        (true, None) => {
            let fds: Vec<_> = listen_fds(true)
                .context("while getting LISTEN_FDS")?
                .iter()
                .collect();
            if fds.is_empty() {
                Err(anyhow!("expected at least one LISTEN_FD"))
            } else {
                for fd in fds.clone() {
                    is_socket(fd, None, None, Listening::IsListening)
                        .with_context(|| format!("fd{} is not a listening socket", fd))?;
                }
                Ok(fds.into_iter().map(ListenOn::Raw).collect())
            }
        }
        (false, Some(port)) => Ok(vec![ListenOn::Port(port)]),
        _ => Err(anyhow!("--systemd-socket and --port cannot both be set")),
    }?;

    let executions: Vec<_> = listen_on
        .into_iter()
        .map(|socket| facebook::run(log.clone(), fb.clone(), metald.clone(), socket))
        .collect();
    try_join_all(executions).await?;
    Ok(())
}
