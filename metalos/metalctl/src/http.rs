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

use anyhow::{Context, Result};
use bytes::{Buf, Bytes};
use futures_core::stream::Stream;
use futures_util::StreamExt;
use hyper::client::connect::dns::Name;
use hyper::client::{Client, HttpConnector};
use hyper_rustls::HttpsConnector;
use rustls::ClientConfig;
use slog::{warn, Logger};
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

#[cfg(test)]
mod tests {
    use super::Resolver;
    use anyhow::Result;
    use hyper::Uri;
    use tower::Service;

    #[tokio::test]
    async fn resolves_raw_ip() -> Result<()> {
        let mut resolver = Resolver::new()?;
        let addrs: Vec<_> = resolver.call("::1".parse().unwrap()).await?.collect();
        assert_eq!(addrs, vec!["[::1]:443".parse().unwrap()]);
        Ok(())
    }

    #[tokio::test]
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
