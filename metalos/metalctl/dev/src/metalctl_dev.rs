/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::marker::PhantomData;
use std::str::FromStr;

use anyhow::{Context, Error, Result};
use slog::{o, Logger};
use structopt::StructOpt;
use uuid::Uuid;

use metalos_host_configs::packages::{Format, Kind, Package};

mod offline;
mod service;

#[derive(StructOpt)]
#[structopt(no_version)]
enum Opts {
    /// Manage MetalOS kernel and rootfs versions
    Offline(offline::Opts),
    /// Manually manage MetalOS Native Services
    Service(service::Opts),
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::from_args();
    let log = Logger::root(slog_glog_fmt::default_drain(), o!());
    match opts {
        Opts::Offline(opts) => offline::offline(log, opts).await,
        Opts::Service(opts) => service::service(log, opts).await,
    }
}

pub(crate) trait FormatArg {
    fn format() -> Format;
}

pub(crate) struct SendstreamPackage;

impl FormatArg for SendstreamPackage {
    fn format() -> Format {
        Format::Sendstream
    }
}

pub(crate) struct FilePackage;

impl FormatArg for FilePackage {
    fn format() -> Format {
        Format::File
    }
}

pub(crate) struct PackageArg<F: FormatArg> {
    name: String,
    uuid: Uuid,
    format: PhantomData<F>,
}

impl<F: FormatArg> FromStr for PackageArg<F> {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        let (name, uuid) = s.split_once(':').context("must have exactly one ':'")?;
        Ok(Self {
            name: name.to_string(),
            uuid: uuid.parse().context("invalid uuid")?,
            format: PhantomData,
        })
    }
}

impl<F, K> From<PackageArg<F>> for Package<K, Uuid>
where
    F: FormatArg,
    K: Kind,
{
    fn from(pkg: PackageArg<F>) -> Self {
        Self::new(pkg.name, pkg.uuid, None, F::format())
    }
}
