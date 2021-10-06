/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::{Path, PathBuf};

use anyhow::{bail, Context, Result};
use bytes::Buf;
use hyper::header::LOCATION;
use hyper::{StatusCode, Uri};
use slog::{debug, info, o, Logger};
use structopt::StructOpt;

use evalctx::{Generator, Host};

#[derive(StructOpt)]
pub struct Opts {
    host_config_uri: Uri,
    root: PathBuf,
}

pub async fn apply_host_config(log: Logger, opts: Opts) -> Result<()> {
    let log = log.new(o!("host-config-uri" => opts.host_config_uri.to_string(), "root" => opts.root.display().to_string()));

    let client = crate::http::client(log.clone()).context("failed to create https client")?;

    // hyper is a low level client (which is good for our dns connector), but
    // then we have to do things like follow redirects manually
    let mut uri = opts.host_config_uri;
    let mut redirects = 0u8;
    let resp = loop {
        let resp = client.get(uri.clone()).await?;
        if resp.status().is_redirection() {
            let mut new_uri = resp.headers()[LOCATION]
                .to_str()?
                .parse::<Uri>()
                .context("invalid redirect uri")?
                .into_parts();
            if new_uri.scheme.is_none() {
                new_uri.scheme = uri.scheme().map(|s| s.to_owned());
            }
            if new_uri.authority.is_none() {
                new_uri.authority = uri.authority().map(|a| a.to_owned());
            }
            let new_uri = Uri::from_parts(new_uri)?;
            debug!(log, "redirected from {:?} to {:?}", uri, new_uri);
            uri = new_uri;
            redirects += 1;
            if redirects > 10 {
                bail!("too many redirects");
            }
            continue;
        }
        info!(log, "downloading image from {:?}", uri);
        break resp;
    };

    let status = resp.status();
    if status != StatusCode::OK {
        bail!("http response was not OK: {:?}", status);
    }
    let body = hyper::body::aggregate(resp.into_body())
        .await
        .context("failed to load json body")?;
    let host: Host =
        serde_json::from_reader(body.reader()).context("failed to deserialize host json")?;
    let generators = Generator::load(Path::new("/usr/lib/metalos/generators"))
        .context("failed to load generators from /usr/lib/metalos/generators")?;
    for gen in generators {
        let output = gen.eval(&host)?;
        output.apply(&opts.root)?;
    }
    Ok(())
}
