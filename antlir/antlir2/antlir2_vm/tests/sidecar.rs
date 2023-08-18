/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::net::IpAddr;

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use warp::Filter;

#[derive(Parser, Debug)]
struct Cli {
    #[clap(long, default_value_t = 8000)]
    port: u16,
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let hi = warp::path("hello").map(move || cli.port.to_string());
    let addr = IpAddr::V6("::".parse().context("Failed to get server address")?);
    warp::serve(hi).run((addr, cli.port)).await;
    Ok(())
}
