/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use fbthrift::binary_protocol::deserialize as binary_deserialize;
use fbthrift::simplejson_protocol::deserialize as json_deserialize;
use metalos_host_configs::host::HostConfig;
use reqwest::header::HeaderValue;
use reqwest::Client;
use std::path::Path;
use url::Url;

static THRIFT_CONTENT_TYPE: HeaderValue = HeaderValue::from_static("application/x-thrift");

pub fn client() -> Result<Client> {
    Client::builder()
        .use_rustls_tls()
        .build()
        .context("building client")
}

pub fn get_host_config_from_file(path: &Path) -> Result<HostConfig> {
    let blob =
        std::fs::read(path).with_context(|| format!("while opening file {}", path.display()))?;
    match json_deserialize(&blob).context("while deserializing json thrift") {
        Ok(hc) => Ok(hc),
        Err(json_err) => binary_deserialize(blob)
            .context("while deserializing binary thrift")
            .context(json_err),
    }
}

pub async fn get_host_config(uri: &Url) -> Result<HostConfig> {
    match uri.scheme() {
        "http" | "https" => {
            let client = client()?;
            let resp = client
                .get(uri.clone())
                .header(http::header::CONTENT_TYPE, THRIFT_CONTENT_TYPE.clone())
                .send()
                .await
                .with_context(|| format!("while GETting {}", uri))?;
            let content_type: Option<HeaderValue> =
                resp.headers().get(http::header::CONTENT_TYPE).cloned();
            let bytes = resp
                .bytes()
                .await
                .context("while downloading response body")?;
            if Some(&THRIFT_CONTENT_TYPE) == content_type.as_ref() {
                binary_deserialize(bytes).context("while deserializing binary thrift")
            } else {
                json_deserialize(bytes).context("while deserializing json thrift")
            }
        }
        "file" => get_host_config_from_file(Path::new(uri.path())),
        scheme => Err(anyhow!("Unsupported scheme {} in {:?}", scheme, uri)),
    }
}
