/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::io::RawFd;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;
use futures::future::try_join_all;
use plain_systemd::daemon::is_socket;
use plain_systemd::daemon::listen_fds;
use plain_systemd::daemon::Listening;

mod acl;
mod thrift_server;
mod update;

#[cfg(facebook)]
mod facebook;

use acl::PermissionsChecker;
use thrift_server::Metald;

#[derive(Debug, Parser)]
#[clap(name = "Metald Thrift Service")]
struct Arguments {
    #[clap(long, group = "listen")]
    systemd_socket: bool,
    /// Port to serve traffic on
    #[clap(short, long, group = "listen", help = "Listen on TCP port")]
    port: Option<u16>,
    #[clap(long, env = "METALD_NO_ACL_CHECK", help = "disable acl checking")]
    no_acl_check: bool,
}

pub(crate) enum ListenOn {
    Raw(RawFd),
    Port(u16),
}

pub async fn start_service(fb: FacebookInit, netos: bool) -> Result<()> {
    // Process commandline flags
    let args = Arguments::parse();

    let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());

    let acl_checker: Arc<Box<dyn PermissionsChecker<Identity = identity::Identity>>> = match args
        .no_acl_check
    {
        true => Arc::new(Box::new(crate::acl::AllowAll::new())),
        false => {
            #[cfg(not(facebook))]
            unimplemented!("OSS acl checking not yet implemented");
            #[cfg(facebook)]
            {
                let mut checkers: Vec<Box<dyn PermissionsChecker<Identity = identity::Identity>>> =
                    Vec::new();
                // We put the IdentityChecker before the ACLChecker, as the former check can be
                // completed entirely locally. If it passes, we never have to issue an RPC via
                // ACLChecker.
                checkers.push(Box::new(
                    crate::facebook::acl::new_identity_checker()
                        .context("while creating identity checker")?,
                ));
                checkers.push(Box::new(
                    crate::facebook::acl::new_acl_checker(fb)
                        .context("while creating fb acl checker")?,
                ));
                Arc::new(Box::new(
                    crate::facebook::fallback_acl_checker::new_fallback_acl_checker(checkers),
                ))
            }
        }
    };

    let metald = Metald::new(fb, log.clone(), acl_checker)?;

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
        .map(|socket| facebook::run(log.clone(), fb.clone(), metald.clone(), socket, netos))
        .collect();
    try_join_all(executions).await?;
    Ok(())
}
