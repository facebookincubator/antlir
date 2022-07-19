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
use systemd::daemon::is_socket;
use systemd::daemon::listen_fds;
use systemd::daemon::Listening;

use fbinit::FacebookInit;

mod thrift_server;
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

    let metald = Metald { fb };

    let listen_on = match (args.systemd_socket, args.port) {
        (true, None) => {
            let mut fds: Vec<_> = listen_fds(true)
                .context("while getting LISTEN_FDS")?
                .iter()
                .collect();
            if fds.len() != 1 {
                Err(anyhow!("expected exactly one LISTEN_FD"))
            } else {
                let fd = fds.pop().expect("already validated this has one element");
                match is_socket(fd, None, None, Listening::IsListening)
                    .context("while checking LISTEN_FD socket properties")?
                {
                    true => Ok(ListenOn::Raw(fd)),
                    false => Err(anyhow!("LISTEN_FD is not a listening socket")),
                }
            }
        }
        (false, Some(port)) => Ok(ListenOn::Port(port)),
        _ => Err(anyhow!("--systemd-socket and --port cannot both be set")),
    }?;

    facebook::run(log, fb, metald, listen_on).await
}
