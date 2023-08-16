/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::net::IpAddr;

use anyhow::Context;
use anyhow::Result;
use warp::Filter;

#[tokio::main]
async fn main() -> Result<()> {
    let hi = warp::path("hello").map(|| "world");
    let addr = IpAddr::V6("::".parse().context("Failed to get server address")?);
    warp::serve(hi).run((addr, 8000)).await;
    Ok(())
}
