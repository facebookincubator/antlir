/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::OpenOptions;
use std::path::PathBuf;

use anyhow::{Context, Result};
use structopt::StructOpt;
use url::Url;

use get_host_config::get_host_config;

#[derive(StructOpt)]
pub struct Opts {
    uri: Url,
    out: PathBuf,
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
