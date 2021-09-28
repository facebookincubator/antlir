/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use hyper::header::{CONTENT_LENGTH, LOCATION};
use hyper::{StatusCode, Uri};
use slog::{debug, info, o, Logger};
use structopt::StructOpt;

#[derive(StructOpt)]
pub struct Opts {
    package: String,
    dest: PathBuf,
    #[structopt(long)]
    download_only: bool,
    #[structopt(long)]
    decompress_download: bool,
}

use crate::http::{drain_stream, https_trustdns_connector};

pub async fn fetch_image(log: Logger, config: crate::Config, opts: Opts) -> Result<()> {
    let log = log.new(o!("package" => opts.package.clone(), "dest" => format!("{:?}", opts.dest)));
    fs::create_dir_all(&opts.dest)
        .with_context(|| format!("failed to create destination dir {:?}", opts.dest))?;

    let mut uri = config.download.package_uri(opts.package)?;
    debug!(log, "downloading from {}", uri);

    let https = https_trustdns_connector()?;
    let client: hyper::Client<_, hyper::Body> = hyper::Client::builder().build(https);

    // hyper is a low level client (which is good for our dns connector), but
    // then we have to do things like follow redirects manually
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
    if let Some(content_len) = resp.headers().get(CONTENT_LENGTH) {
        if let Ok(len) = content_len.to_str().unwrap_or("").parse::<u64>() {
            debug!(log, "image is {} bytes", len);
        }
    }
    let body = resp.into_body();

    if opts.download_only {
        debug!(log, "downloading image as file");
        let dst = fs::File::create(opts.dest.join("download"))?;
        let mut dst = BufWriter::new(dst);
        match opts.decompress_download {
            true => {
                let mut decoder = zstd::stream::write::Decoder::new(dst)
                    .context("failed to initialize decompressor")?;
                drain_stream(body, &mut decoder).await?;
                decoder.flush()?;
            }
            false => {
                drain_stream(body, &mut dst).await?;
            }
        };
    } else {
        info!(log, "receiving image as a zstd-compressed sendstream");
        let mut child = Command::new("btrfs")
            .args(&[&"receive".into(), &opts.dest])
            .stdin(Stdio::piped())
            .spawn()
            .context("btrfs receive command failed to start")?;
        let stdin = child.stdin.take().unwrap();
        let mut decoder = zstd::stream::write::Decoder::new(BufWriter::new(stdin))
            .context("failed to initialize decompressor")?;
        drain_stream(body, &mut decoder).await?;
        decoder.flush()?;
    }
    Ok(())
}
