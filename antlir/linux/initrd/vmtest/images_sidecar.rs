/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
use std::env;
use std::net::IpAddr;
use std::path::PathBuf;

use anyhow::Result;
use warp::Filter;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    let images_dir: PathBuf = (&args[1]).into();

    let log = warp::log::custom(|info| {
        eprintln!(
            "images_sidecar: {} {} {}",
            info.method(),
            info.path(),
            info.status(),
        );
    });

    // TODO this should eventually test with multiple images + multiple
    // versions, but for the first pass just always return the metalos
    // sendstream
    let routes = warp::path!("package" / "metalos:1")
        .and(warp::filters::fs::file(
            images_dir.join("metalos.sendstream.zst"),
        ))
        .with(log);

    let addr = IpAddr::V6("::".parse()?);
    warp::serve(routes).run((addr, 8000)).await;
    Ok(())
}
