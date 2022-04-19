/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

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
    // We want to use the actual thrift<->json serializer, but we also want pretty printing
    // Serialize with fbthrift so any thrift-specific serializing logic runs
    let json = fbthrift::simplejson_protocol::serialize(&c);
    // Then re-parse into arbitrary JSON and re-encode but pretty-printed
    let re_decoded: serde_json::Value = serde_json::from_slice(&json).context("parsing JSON")?;
    let prettyfied = serde_json::to_string_pretty(&re_decoded).context("prettifying JSON")?;
    std::fs::write(&opts.out, &prettyfied)
        .with_context(|| format!("while writing host config to {:?}", opts.out))
}
