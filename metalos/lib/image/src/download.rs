use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::{Client, Url};
use slog::{debug, Logger};
use std::io::ErrorKind;
use std::pin::Pin;
use thiserror::Error;
use uuid::Uuid;

use crate::Result;
use btrfs::sendstream::{Sendstream, SendstreamExt, Zstd};
use metalos_host_configs::packages::{Kind, Package};

#[derive(Error, Debug)]
pub enum Error {
    #[error("failure while setting up client {0}")]
    InitClient(reqwest::Error),
    #[error("format uri '{uri}' is badly formed: {error}")]
    InvalidUri { uri: String, error: anyhow::Error },
    #[error("failure while opening http connection {0}")]
    Open(reqwest::Error),
    #[error("Got non-success status code {0}. Body was {1}")]
    StatusCode(reqwest::StatusCode, String),
}

#[async_trait]
pub trait Downloader {
    type BytesStream: Stream<Item = std::io::Result<Bytes>> + Unpin + Send;
    type Sendstream: SendstreamExt;

    /// Open a bytes stream from the underlying image source.
    async fn open_bytes_stream<K: Kind>(
        &self,
        log: Logger,
        package: &Package<K, Uuid>,
    ) -> Result<Self::BytesStream>;

    /// Open a [Sendstream] from the underlying image source.
    async fn open_sendstream<K: Kind>(
        &self,
        log: Logger,
        package: &Package<K, Uuid>,
    ) -> Result<Self::Sendstream>;
}

#[derive(Clone)]
pub struct HttpsDownloader {
    client: Client,
}

static FORMAT_URI: &str = {
    #[cfg(facebook)]
    {
        "https://fbpkg.fbinfra.net/fbpkg/{package}"
    }
    #[cfg(not(facebook))]
    {
        "https://metalos/package/{package}"
    }
};

impl HttpsDownloader {
    pub fn new() -> Result<Self> {
        // TODO: it would be nice to restrict to https only, but we use plain
        // http for tests, and https doesn't do much for security compared to
        // something like checking image signatures
        let client = reqwest::Client::builder()
            .trust_dns(true)
            .user_agent("metalos::image/1")
            .build()
            .map_err(Error::InitClient)?;
        Ok(Self { client })
    }

    pub fn package_url<K: Kind>(&self, id: &Package<K, Uuid>) -> Result<Url> {
        match &id.override_uri {
            Some(u) => Ok(u.clone()),
            None => {
                let uri = FORMAT_URI.replace("{package}", &id.identifier());
                Url::parse(&uri).map_err(|e| {
                    Error::InvalidUri {
                        uri,
                        error: e.into(),
                    }
                    .into()
                })
            }
        }
    }
}

impl From<HttpsDownloader> for Client {
    fn from(h: HttpsDownloader) -> Client {
        h.client
    }
}

#[async_trait]
impl Downloader for &HttpsDownloader {
    type BytesStream = Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>;
    type Sendstream = Sendstream<Zstd, Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>>;

    async fn open_bytes_stream<K: Kind>(
        &self,
        log: Logger,
        id: &Package<K, Uuid>,
    ) -> Result<Self::BytesStream> {
        let url = self.package_url(id)?;
        debug!(log, "{:?} -> {}", id, url);
        let response = self
            .client
            .get(self.package_url(id)?)
            .send()
            .await
            .map_err(Error::Open)?;

        if response.status().is_success() {
            Ok(Box::pin(response.bytes_stream().map(|r| {
                r.map_err(|e| std::io::Error::new(ErrorKind::Other, e))
            })))
        } else {
            Err(Error::StatusCode(
                response.status(),
                match response.text().await {
                    Ok(body) => body,
                    Err(e) => format!("Failed to ready body: {:?}", e),
                },
            )
            .into())
        }
    }

    async fn open_sendstream<K: Kind>(
        &self,
        log: Logger,
        package: &Package<K, Uuid>,
    ) -> Result<Self::Sendstream> {
        let stream = self.open_bytes_stream(log, package).await?;
        Ok(Sendstream::new(stream))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use metalos_host_configs::packages::{Format, Rootfs};

    #[test]
    fn image_url() -> anyhow::Result<()> {
        let h = HttpsDownloader::new()?;
        assert_eq!(
            format!(
                "{}/abc:deadbeefdeadbeefdeadbeefdeadbeef",
                FORMAT_URI.replace("{package}", "").trim_end_matches('/')
            ),
            String::from(h.package_url(&Rootfs::new(
                "abc".into(),
                "deadbeefdeadbeefdeadbeefdeadbeef".parse().unwrap(),
                None,
                Format::Sendstream,
            ))?)
        );
        Ok(())
    }
}
