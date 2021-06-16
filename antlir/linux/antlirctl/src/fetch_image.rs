/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::io::{copy, BufWriter, Write};
use std::net::SocketAddr;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Error, Result};
use bytes::{Buf, Bytes};
use futures_core::stream::Stream;
use futures_util::StreamExt;
use hyper::client::connect::dns::Name;
use hyper::client::HttpConnector;
use hyper::header::{CONTENT_LENGTH, LOCATION};
use hyper::{StatusCode, Uri};
use hyper_rustls::HttpsConnector;
use rustls::ClientConfig;
use slog::{debug, info, o, Logger};
use structopt::StructOpt;
use tower::Service;
use trust_dns_resolver::TokioAsyncResolver;

#[derive(StructOpt)]
pub struct Opts {
    url: Uri,
    dest: PathBuf,
    #[structopt(long)]
    download_only: bool,
    #[structopt(long)]
    decompress_download: bool,
}

// This return type is nasty, but lets it be separate from the callsite and
// enables re-use of this trust-dns resolver and hyper https connector.
// Unfortunately using trust-dns is our best option compared to getaddrinfo,
// which would require more messing around with glibc and platforms than I care
// to engage in now.
fn https_trustdns_connector() -> Result<
    HttpsConnector<
        HttpConnector<
            impl Service<
                Name,
                Error = anyhow::Error,
                Future = impl Send,
                Response = impl Iterator<Item = SocketAddr>,
            > + Clone
            + Send,
        >,
    >,
> {
    let async_resolver = std::sync::Arc::new(
        TokioAsyncResolver::tokio_from_system_conf()
            .context("failed to create trust-dns resolver")?,
    );
    let resolver = tower::service_fn(move |name: Name| {
        let async_resolver = async_resolver.clone();
        async move {
            let lookup = async_resolver
                .lookup_ip(name.as_str())
                .await
                .with_context(|| format!("failed to lookup hostname '{}'", name))?;
            Ok(lookup
                .iter()
                .map(|a| SocketAddr::from((a, 443_u16)))
                .collect::<Vec<_>>()
                .into_iter())
        }
    });
    let mut http = HttpConnector::new_with_resolver(resolver);
    http.enforce_http(false);
    let mut config = ClientConfig::new();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    config.root_store = rustls_native_certs::load_native_certs()
        .map_err(|_| Error::msg("failed to load native root cert store"))?;
    Ok((http, config).into())
}

async fn drain_stream<S: Stream<Item = hyper::Result<Bytes>>, W: Write>(
    stream: S,
    mut writer: W,
) -> Result<()> {
    tokio::pin!(stream);
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.context("failed to get chunk")?;
        copy(&mut chunk.reader(), &mut writer).context("failed to write chunk")?;
    }
    Ok(())
}

pub async fn fetch_image(log: Logger, opts: Opts) -> Result<()> {
    let log = log.new(o!("url" => opts.url.to_string(), "dest" => format!("{:?}", opts.dest)));
    fs::create_dir_all(&opts.dest)
        .with_context(|| format!("failed to create destination dir {:?}", opts.dest))?;

    match opts.url.scheme_str() {
        Some("http") | Some("https") => {}
        _ => bail!("only http(s) urls are supported"),
    };

    let https = https_trustdns_connector()?;
    let client: hyper::Client<_, hyper::Body> = hyper::Client::builder().build(https);

    // hyper is a low level client (which is good for our dns connector), but
    // then we have to do things like follow redirects manually
    let mut redirects = 0u8;
    let mut uri = opts.url.clone();
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
