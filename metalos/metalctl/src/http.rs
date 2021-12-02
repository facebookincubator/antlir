/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::future::Future;
use std::io::{copy, Write};
use std::net::SocketAddr;
use std::pin::Pin;
use std::task::Poll;

use anyhow::{bail, Context, Result};
use bytes::{Buf, Bytes};
use futures_core::stream::Stream;
use futures_util::StreamExt;
use hyper::client::connect::dns::Name;
use hyper::client::{Client, HttpConnector};
use hyper::header::{CONTENT_LENGTH, LOCATION};
use hyper::{body, Body, Response, StatusCode, Uri};
use hyper_rustls::HttpsConnector;
use rustls::ClientConfig;
use slog::{debug, info, warn, Logger};
use tower::Service;
use trust_dns_resolver::TokioAsyncResolver;

#[derive(Clone)]
pub struct Resolver(std::sync::Arc<TokioAsyncResolver>);

impl Resolver {
    pub fn new() -> Result<Self> {
        let async_resolver = std::sync::Arc::new(
            TokioAsyncResolver::tokio_from_system_conf()
                .context("failed to create trust-dns resolver")?,
        );
        Ok(Self(async_resolver))
    }
}

impl Service<Name> for Resolver {
    type Response = std::vec::IntoIter<SocketAddr>;
    type Future = Pin<Box<dyn Future<Output = Result<Self::Response, Self::Error>> + Send>>;
    type Error = anyhow::Error;

    fn poll_ready(&mut self, _cx: &mut std::task::Context<'_>) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn call(&mut self, name: Name) -> Self::Future {
        let resolver = self.0.clone();
        Box::pin((|| async move {
            let lookup = resolver
                .lookup_ip(name.as_str())
                .await
                .with_context(|| format!("failed to lookup hostname '{}'", name))?;
            Ok(lookup
                .iter()
                .map(|a| SocketAddr::from((a, 443_u16)))
                .collect::<Vec<_>>()
                .into_iter())
        })())
    }
}

fn https_trustdns_connector(log: Logger) -> Result<HttpsConnector<HttpConnector<Resolver>>> {
    let resolver = Resolver::new()?;
    let mut http = HttpConnector::new_with_resolver(resolver);
    http.enforce_http(false);
    let mut config = ClientConfig::new();
    config.alpn_protocols = vec![b"h2".to_vec(), b"http/1.1".to_vec()];
    config.root_store = rustls_native_certs::load_native_certs().or_else(|e| match e.0 {
        Some(partial_store) => {
            warn!(
                log,
                "only able to load some of the native cert store, proceeding optimistically: {:?}",
                e.1
            );
            Ok(partial_store)
        }
        None => Err(e.1).context("failed to load native root cert store"),
    })?;
    Ok((http, config).into())
}

pub fn client(log: Logger) -> Result<Client<HttpsConnector<HttpConnector<Resolver>>>> {
    let connector = https_trustdns_connector(log)?;
    Ok(Client::builder().build(connector))
}

pub async fn drain_stream<S: Stream<Item = hyper::Result<Bytes>>, W: Write>(
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

// deals with making the http request and redirection retries.
async fn _http_get(log: Logger, mut uri: Uri) -> Result<Response<Body>> {
    let client = client(log.clone()).context("failed to create https client")?;
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
        info!(log, "making http request for {:?}", uri);
        break resp;
    };
    Ok(resp)
}

pub async fn download_file(log: Logger, uri: Uri) -> Result<Body> {
    let resp = _http_get(log.clone(), uri).await?;
    let status = resp.status();
    if status != StatusCode::OK {
        let body_bytes = body::to_bytes(resp.into_body()).await?;
        let body = String::from_utf8(body_bytes.to_vec()).expect("response was not valid utf-8");
        bail!("http response was not OK: {:?}.\n{}", status, body);
    }
    if let Some(content_len) = resp.headers().get(CONTENT_LENGTH) {
        if let Ok(len) = content_len.to_str().unwrap_or("").parse::<u64>() {
            debug!(log, "response body size is {} bytes", len);
        }
    }
    Ok(resp.into_body())
}

pub async fn get(log: Logger, uri: Uri) -> Result<Body> {
    let resp = _http_get(log.clone(), uri).await?;
    let status = resp.status();
    if status != StatusCode::OK {
        let body_bytes = body::to_bytes(resp.into_body()).await?;
        let body = String::from_utf8(body_bytes.to_vec()).expect("response was not valid utf-8");
        bail!("http response was not OK: {:?}.\n{}", status, body);
    }
    if let Some(content_len) = resp.headers().get(CONTENT_LENGTH) {
        if let Ok(len) = content_len.to_str().unwrap_or("").parse::<u64>() {
            debug!(log, "response body size is {} bytes", len);
        }
    }
    Ok(resp.into_body())
}

#[cfg(test)]
mod tests {
    use super::Resolver;
    use anyhow::Result;
    use hyper::Uri;
    use tower::Service;

    #[test]
    async fn resolves_raw_ip() -> Result<()> {
        let mut resolver = Resolver::new()?;
        let addrs: Vec<_> = resolver.call("::1".parse().unwrap()).await?.collect();
        assert_eq!(addrs, vec!["[::1]:443".parse().unwrap()]);
        Ok(())
    }

    #[test]
    async fn connect_to_raw_ip() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), slog::o!());
        let client = super::client(log)?;
        let response = client.get(Uri::from_static("https://[::1]/")).await;
        // no matter whether or not anything in listening for https on the
        // localhost, we can determine if the connector is working or not
        // if the response is ok, then the connector definitely works
        if response.is_err() {
            // if the request failed, we want a connection refused error, to
            // show that the dns resolver worked, and there was just nothing at
            // the other end
            assert!(response.unwrap_err().is_connect());
        }
        Ok(())
    }
}
