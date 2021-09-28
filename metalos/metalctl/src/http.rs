/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::{copy, Write};
use std::net::SocketAddr;

use anyhow::{Context, Error, Result};
use bytes::{Buf, Bytes};
use futures_core::stream::Stream;
use futures_util::StreamExt;
use hyper::client::connect::dns::Name;
use hyper::client::HttpConnector;
use hyper_rustls::HttpsConnector;
use rustls::ClientConfig;
use tower::Service;
use trust_dns_resolver::TokioAsyncResolver;

// This return type is nasty, but lets it be separate from the callsite and
// enables re-use of this trust-dns resolver and hyper https connector.
// Unfortunately using trust-dns is our best option compared to getaddrinfo,
// which would require more messing around with glibc and platforms than I care
// to engage in now.
pub fn https_trustdns_connector() -> Result<
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
