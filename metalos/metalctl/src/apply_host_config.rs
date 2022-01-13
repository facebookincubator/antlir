/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use anyhow::{anyhow, Context, Result};
use slog::{o, Logger};
use structopt::StructOpt;
use url::Url;

use evalctx::{Generator, Host};

#[derive(StructOpt)]
pub struct Opts {
    host_config_uri: Url,
    root: PathBuf,
}

pub async fn apply_host_config(log: Logger, opts: Opts) -> Result<()> {
    let log = log.new(o!("host-config-uri" => opts.host_config_uri.to_string(), "root" => opts.root.display().to_string()));

    let host: Host = match opts.host_config_uri.scheme() {
        "http" | "https" => {
            let client = crate::http::client()?;
            client
                .get(opts.host_config_uri.clone())
                .send()
                .await
                .with_context(|| format!("while GETting {}", opts.host_config_uri))?
                .json()
                .await
                .context("while parsing host json")
        }
        "file" => {
            let f = std::fs::File::open(opts.host_config_uri.path())
                .with_context(|| format!("while opening file {}", opts.host_config_uri.path()))?;
            serde_json::from_reader(f).context("while deserializing json")
        }
        scheme => Err(anyhow!(
            "Unsupported scheme {} in {:?}",
            scheme,
            opts.host_config_uri
        )),
    }?;

    let generators = Generator::load("/usr/lib/metalos/generators")
        .context("failed to load generators from /usr/lib/metalos/generators")?;
    for gen in generators {
        let output = gen.eval(&host)?;
        output.apply(log.clone(), &opts.root)?;
    }
    Ok(())
}
