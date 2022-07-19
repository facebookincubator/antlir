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
            let mut fds: Vec<_> = sd_listen_fds();
            if fds.len() != 1 {
                Err(anyhow!("expected exactly one LISTEN_FD"))
            } else {
                let fd = fds.pop().expect("already validated this has one element");
                Ok(ListenOn::Raw(fd))
            }
        }
        (false, Some(port)) => Ok(ListenOn::Port(port)),
        _ => Err(anyhow!("--systemd-socket and --port cannot both be set")),
    }?;

    facebook::run(log, fb, metald, listen_on).await
}

fn sd_listen_fds() -> Vec<RawFd> {
    // rust implementation of sd_listen_fds
    // https://www.freedesktop.org/software/systemd/man/sd_listen_fds.html
    // check and return the value in the env LISTEN_FDS, that is the file descriptor
    // reported by the systemd .socket. The server uses this fd to bring up the server with the same socket.
    const LISTEN_FDS_START: RawFd = 3;
    let fds: Vec<RawFd> = if let Some(count) = std::env::var("LISTEN_FDS")
        .ok()
        .and_then(|x| x.parse().ok())
    {
        (0..count).map(|offset| LISTEN_FDS_START + offset).collect()
    } else {
        Vec::new()
    };
    fds
}
