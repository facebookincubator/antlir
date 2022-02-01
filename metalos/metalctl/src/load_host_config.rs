/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::OpenOptions;
use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use structopt::StructOpt;
use url::Url;

use host::types::HostConfig;

#[derive(StructOpt)]
pub struct Opts {
    uri: Url,
    out: PathBuf,
}

pub async fn get_host_config(uri: &Url) -> Result<HostConfig> {
    match uri.scheme() {
        "http" | "https" => {
            let client = crate::http::client()?;
            client
                .get(uri.clone())
                .send()
                .await
                .with_context(|| format!("while GETting {}", uri))?
                .json()
                .await
                .context("while parsing host json")
        }
        "file" => {
            let f = std::fs::File::open(uri.path())
                .with_context(|| format!("while opening file {}", uri.path()))?;
            serde_json::from_reader(f).context("while deserializing json")
        }
        scheme => Err(anyhow!("Unsupported scheme {} in {:?}", scheme, uri)),
    }
}

pub async fn load_host_config(opts: Opts) -> Result<()> {
    let c = get_host_config(&opts.uri)
        .await
        .with_context(|| format!("while downloading host config from {}", &opts.uri))?;
    let out = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(&opts.out)
        .with_context(|| format!("while opening {:?} for writing", &opts.out))?;
    serde_json::to_writer(out, &c)
        .with_context(|| format!("while writing host config to {:?}", opts.out))
}
