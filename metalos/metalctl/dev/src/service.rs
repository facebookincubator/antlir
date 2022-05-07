/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */
use std::str::FromStr;

use anyhow::{Context, Error, Result};
use slog::Logger;
use structopt::StructOpt;
use uuid::Uuid;

use metalos_host_configs::packages::{Format, Service as ServicePackage};
use metalos_host_configs::runtime_config::Service;
use package_download::{ensure_package_on_disk, HttpsDownloader};
use service::{ServiceSet, Transaction};
use systemd::Systemd;

#[derive(StructOpt)]
pub(crate) enum Opts {
    /// Start specific versions of a set of native services, replacing the
    /// running version if necessary.
    Start(Start),
    /// Stop a set of native services
    Stop(Stop),
}

struct ServiceID(String, Uuid);

impl FromStr for ServiceID {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let (name, uuid) = s.split_once(':').context("must have exactly one ':'")?;
        Ok(Self(
            name.to_string(),
            uuid.parse().context("invalid uuid")?,
        ))
    }
}

impl From<&ServiceID> for Service {
    fn from(sid: &ServiceID) -> Service {
        Service {
            svc: ServicePackage::new(sid.0.clone(), sid.1, None, Format::Sendstream),
            config_generator: None,
        }
    }
}

#[derive(StructOpt)]
pub(crate) struct Start {
    services: Vec<ServiceID>,
}

#[derive(StructOpt)]
pub(crate) struct Stop {
    services: Vec<String>,
}

pub(crate) async fn service(log: Logger, opts: Opts) -> Result<()> {
    let sd = Systemd::connect(log.clone()).await?;
    match opts {
        Opts::Start(start) => {
            let dl = HttpsDownloader::new().context("while creating downloader")?;
            for svc in &start.services {
                let svc: Service = svc.into();
                ensure_package_on_disk(log.clone(), &dl, svc.svc).await?;
            }

            let mut set = ServiceSet::current(&sd).await?;
            for svc in start.services {
                set.insert(svc.0, svc.1);
            }
            let tx = Transaction::with_next(&sd, set).await?;
            tx.commit(log, &sd).await?;
        }
        Opts::Stop(stop) => {
            let mut set = ServiceSet::current(&sd).await?;
            for name in stop.services {
                set.remove(&name);
            }
            let tx = Transaction::with_next(&sd, set).await?;
            tx.commit(log, &sd).await?;
        }
    }
    Ok(())
}
