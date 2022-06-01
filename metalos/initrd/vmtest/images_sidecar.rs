/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
use std::collections::HashMap;
use std::env;
use std::net::IpAddr;
use std::path::PathBuf;

use anyhow::Result;
use warp::Filter;

#[tokio::main]
async fn main() -> Result<()> {
    let args: Vec<_> = env::args().collect();
    let images_dir: PathBuf = (&args[1]).into();

    eprintln!("images_sidecar: serving the following packages");
    for entry in std::fs::read_dir(&images_dir)? {
        if let Ok(entry) = entry {
            eprintln!(
                "images_sidecar:  {}",
                entry.path().strip_prefix(&images_dir)?.display()
            );
        }
    }

    let log = warp::log::custom(|info| {
        eprintln!(
            "images_sidecar: {} {} {}",
            info.method(),
            info.path(),
            info.status(),
        );
    });

    let packages = warp::path("package").and(warp::filters::fs::dir(images_dir));
    let send_event = warp::path("send-event")
        .and(warp::query::<HashMap<String, String>>())
        .map(|query: HashMap<String, String>| {
            eprintln!(
                "images_sidecar: received event {} ({})",
                query.get("name").map_or("None", String::as_str),
                query.get("payload").map_or("None", String::as_str),
            );
            "OK"
        });

    let routes = packages.or(send_event).with(log);

    let addr = IpAddr::V6("::".parse()?);
    warp::serve(routes).run((addr, 8000)).await;
    Ok(())
}
