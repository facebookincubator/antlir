/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{anyhow, Context, Result};
use host::types::HostConfig;
use reqwest::Client;
use std::path::Path;
use url::Url;

pub fn client() -> Result<Client> {
    Client::builder()
        .trust_dns(true)
        .use_rustls_tls()
        .build()
        .context("building client")
}

pub fn get_host_config_from_file(path: &Path) -> Result<HostConfig> {
    let f = std::fs::File::open(path)
        .with_context(|| format!("while opening file {}", path.display()))?;
    serde_json::from_reader(f).context("while deserializing json")
}

pub async fn get_host_config(uri: &Url) -> Result<HostConfig> {
    match uri.scheme() {
        "http" | "https" => {
            let client = client()?;
            client
                .get(uri.clone())
                .send()
                .await
                .with_context(|| format!("while GETting {}", uri))?
                .json()
                .await
                .context("while parsing host json")
        }
        "file" => get_host_config_from_file(Path::new(uri.path())),
        scheme => Err(anyhow!("Unsupported scheme {} in {:?}", scheme, uri)),
    }
}
