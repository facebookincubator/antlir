/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::env;
use std::os::unix::io::RawFd;

use fb303::server::make_FacebookService_server;
use fb303_core::server::make_BaseService_server;
use fbinit::FacebookInit;
use srserver::service_framework::BuildModule;
use srserver::service_framework::Fb303Module;
use srserver::service_framework::ServiceFramework;
use srserver::service_framework::ThriftStatsModule;
use srserver::ThriftServerBuilder;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use futures::StreamExt;
use signal_hook::consts::signal::SIGINT;
use signal_hook::consts::signal::SIGTERM;
use signal_hook_tokio::Signals;
use slog::info;
use slog::Drain;
use slog::Logger;
use tokio::runtime::Runtime;

use metalos_thrift_host_configs::api::server::make_Metalctl_server;
use Metalctl_metadata_sys::create_metadata;

mod thrift_server;
use thrift_server::Metald;

#[cfg(facebook)]
mod facebook;

#[derive(Debug, Parser)]
#[clap(name = "Metald Thrift Service")]
struct Arguments {
    /// Port to serve traffic on
    #[clap(
        short,
        long,
        help = "Execute server using specific port. Default will ignore port value and use the Socket from systemd .socket file",
        default_value = "0"
    )]
    port: u16,
}

#[fbinit::main]
fn main(fb: FacebookInit) -> Result<()> {
    // Process commandline flags
    let args = Arguments::parse();

    let glog_drain = slog_glog_fmt::default_drain();
    let level_drain = slog::LevelFilter::new(glog_drain, slog::Level::Info).fuse();
    let log = slog::Logger::root(level_drain, slog::o!());

    let runtime = Runtime::new()?;

    // TODO duplicating Metald construction will almost definitely make counters
    // harder to implement, but it's temporarily necessary
    let fb303_base = move |proto| make_BaseService_server(proto, Metald { fb });
    let fb303 = move |proto| make_FacebookService_server(proto, Metald { fb }, fb303_base);
    let service = move |proto| make_Metalctl_server(proto, Metald { fb }, fb303);

    let thrift = if args.port == 0 {
        let fd: RawFd = sd_listen_fds().pop().expect("No LISTEN_FD");
        ThriftServerBuilder::new(fb)
            .with_existing_socket(fd) // using systemd .socket
            .with_metadata(create_metadata())
            .with_factory(runtime.handle().clone(), move || service)
            .build()
    } else {
        ThriftServerBuilder::new(fb)
            .with_port(args.port)
            .with_metadata(create_metadata())
            .with_factory(runtime.handle().clone(), move || service)
            .build()
    };

    let mut svc_framework = ServiceFramework::from_server("metald_server", thrift)
        .context("Failed to create service framework server")?;

    svc_framework.add_module(BuildModule)?;
    svc_framework.add_module(ThriftStatsModule)?;
    svc_framework.add_module(Fb303Module)?;

    info!(log, "Starting Metald Thrift service on port: {}", args.port);
    // Start a task to spin up a thrift service
    let thrift_service_handle = runtime.spawn(run_thrift_service(log, svc_framework));
    // Have the runtime wait for thrift service to finish
    runtime.block_on(thrift_service_handle)?
}

async fn run_thrift_service(log: Logger, mut service: ServiceFramework) -> Result<()> {
    let mut signals = Signals::new(&[SIGTERM, SIGINT])?;

    service.serve_background()?;

    signals.next().await;
    info!(log, "Shutting down...");
    service.stop();
    signals.handle().close();
    Ok(())
}

fn sd_listen_fds() -> Vec<RawFd> {
    // rust implementation of sd_listen_fds
    // https://www.freedesktop.org/software/systemd/man/sd_listen_fds.html
    // check and return the value in the env LISTEN_FDS, that is the file descriptor
    // reported by the systemd .socket. The server uses this fd to bring up the server with the same socket.
    const LISTEN_FDS_START: RawFd = 3;
    let fds: Vec<RawFd> =
        if let Some(count) = env::var("LISTEN_FDS").ok().and_then(|x| x.parse().ok()) {
            (0..count).map(|offset| LISTEN_FDS_START + offset).collect()
        } else {
            Vec::new()
        };
    fds
}
