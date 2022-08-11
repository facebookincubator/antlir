/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use fbinit::FacebookInit;
use metalos_thrift_host_configs::api::client::make_Metalctl;
use metalos_thrift_host_configs::api::client::Metalctl;
use slog::Logger;

pub type MetaldClient = Arc<dyn Metalctl + Send + Sync + 'static>;

#[derive(Parser)]
pub(crate) struct MetaldClientOpts {
    #[clap(
        long,
        help = "unix socket to connect to",
        conflicts_with = "metald-host"
    )]
    metald_path: Option<PathBuf>,
    #[clap(long, help = "host:port to connect to", conflicts_with = "metald-path")]
    metald_host: Option<String>,
    #[clap(long, help = "thrift call timeout", default_value = "30s")]
    metald_timeout: humantime::Duration,
    #[clap(long, help = "thrift connection timeout", default_value = "500ms")]
    metald_connect_timeout: humantime::Duration,
}

impl MetaldClientOpts {
    pub(crate) fn client(&self, fb: FacebookInit) -> Result<MetaldClient> {
        let builder = match (&self.metald_path, &self.metald_host) {
            (Some(_), Some(_)) => Err(anyhow!(
                "--metald-path and --metald-host are mutually exclusive"
            )),
            (Some(path), None) => thriftclient::ThriftChannelBuilder::from_path(fb, path),
            (None, Some(hostport)) => thriftclient::ThriftChannelBuilder::from_sock_addr(
                fb,
                hostport
                    .parse()
                    .with_context(|| format!("'{}' is not a valid address", hostport))?,
            ),
            (None, None) => thriftclient::ThriftChannelBuilder::from_path(
                fb,
                metalos_thrift_host_configs::api::consts::SOCKET_PATH,
            ),
        }
        .context("while creating ThriftChannelBuilder")?;
        builder
            .with_conn_timeout(
                self.metald_timeout
                    .as_millis()
                    .try_into()
                    .context("--metald-connect-timeout did not fit into a u32")?,
            )
            .with_recv_timeout(
                self.metald_timeout
                    .as_millis()
                    .try_into()
                    .context("--metald-timeout did not fit into a u32")?,
            )
            .build_client(make_Metalctl)
            .context("while making Metalctl client")
    }
}

#[derive(Parser)]
pub(crate) enum Opts {
    #[cfg(facebook)]
    Status(MetaldClientOpts),
}

pub(crate) async fn run(opts: Opts, _log: Logger, fb: FacebookInit) -> Result<()> {
    match opts {
        #[cfg(facebook)]
        Opts::Status(client_opts) => {
            let client = client_opts.client(fb)?;
            let status = client.getStatus().await?;
            println!("{}", status);
        }
    };
    Ok(())
}
