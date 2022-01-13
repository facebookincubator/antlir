/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{Context, Result};
use reqwest::Client;

pub fn client() -> Result<Client> {
    Client::builder()
        .trust_dns(true)
        .use_rustls_tls()
        .build()
        .context("building client")
}
