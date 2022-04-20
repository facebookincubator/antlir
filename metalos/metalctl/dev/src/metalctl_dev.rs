/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Result;
use slog::{o, Logger};
use structopt::StructOpt;

mod service;

#[derive(StructOpt)]
#[structopt(no_version)]
enum Opts {
    /// Manually manage MetalOS Native Services
    Service(service::Opts),
}

#[tokio::main]
async fn main() -> Result<()> {
    let opts = Opts::from_args();
    let log = Logger::root(slog_glog_fmt::default_drain(), o!());
    match opts {
        Opts::Service(opts) => service::service(log, opts).await,
    }
}
