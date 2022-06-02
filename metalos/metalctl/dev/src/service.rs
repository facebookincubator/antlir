/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{Context, Result};
use slog::Logger;
use structopt::StructOpt;

use metalos_host_configs::packages::{Format, Service as ServicePackage};
use metalos_host_configs::runtime_config::Service;
use package_download::{ensure_package_on_disk, HttpsDownloader};
use service::{ServiceSet, Transaction};
use systemd::Systemd;

use crate::{PackageArg, SendstreamPackage};

#[derive(StructOpt)]
pub(crate) enum Opts {
    /// Start specific versions of a set of native services, replacing the
    /// running version if necessary.
    Start(Start),
    /// Stop a set of native services
    Stop(Stop),
}

impl<F: crate::FormatArg> From<&PackageArg<F>> for Service {
    fn from(sid: &PackageArg<F>) -> Service {
        Service {
            svc: ServicePackage::new(sid.name.clone(), sid.uuid, None, Format::Sendstream),
            config_generator: None,
        }
    }
}

#[derive(StructOpt)]
pub(crate) struct Start {
    services: Vec<PackageArg<SendstreamPackage>>,
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
                set.insert(svc.name, svc.uuid);
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
