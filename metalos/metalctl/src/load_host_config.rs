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
    let json = fbthrift::simplejson_protocol::serialize(&c);
    std::fs::write(&opts.out, json)
        .with_context(|| format!("while writing host config to {:?}", opts.out))
}
