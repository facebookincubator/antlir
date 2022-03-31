use async_trait::async_trait;
use bytes::Bytes;
use futures::{Stream, StreamExt};
use reqwest::{Client, Url};
use std::io::ErrorKind;
use std::pin::Pin;
use thiserror::Error;

use crate::{AnyImage, Result};
use btrfs::sendstream::{Sendstream, SendstreamExt, Zstd};

#[derive(Error, Debug)]
pub enum Error {
    #[error("failure while setting up client {0}")]
    InitClient(reqwest::Error),
    #[error("format uri '{uri}' is badly formed: {error}")]
    InvalidUri { uri: String, error: anyhow::Error },
    #[error("failure while opening http connection {0}")]
    Open(reqwest::Error),
}

#[async_trait]
pub trait Downloader {
    type Sendstream: SendstreamExt;

    /// Open a [Sendstream] from the underlying image source.
    async fn open_sendstream(&self, image: &AnyImage) -> Result<Self::Sendstream>;
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

    pub fn image_url(&self, img: &AnyImage) -> Result<Url> {
        let uri = match &img.override_uri {
            Some(u) => u.clone(),
            None => FORMAT_URI.replace("{package}", &format!("{}:{}", img.name, img.id)),
        };
        Url::parse(&uri).map_err(|e| {
            Error::InvalidUri {
                uri,
                error: e.into(),
            }
            .into()
        })
    }
}

impl From<HttpsDownloader> for Client {
    fn from(h: HttpsDownloader) -> Client {
        h.client
    }
}

#[async_trait]
impl Downloader for HttpsDownloader {
    type Sendstream = Sendstream<Zstd, Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>>;

    async fn open_sendstream(&self, image: &AnyImage) -> Result<Self::Sendstream> {
        let stream = self
            .client
            .get(self.image_url(image)?)
            .send()
            .await
            .map_err(Error::Open)?
            .bytes_stream()
            .map(|r| r.map_err(|e| std::io::Error::new(ErrorKind::Other, e)));
        Ok(Sendstream::new(Box::pin(stream)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use url::ParseError;

    #[test]
    fn image_url() -> anyhow::Result<()> {
        let h = HttpsDownloader::new()?;
        assert_eq!(
            format!(
                "{}/abc:123",
                FORMAT_URI.replace("{package}", "").trim_end_matches('/')
            ),
            String::from(h.image_url(&AnyImage {
                name: "abc".into(),
                id: "123".into(),
                kind: crate::kinds::Kind::Rootfs,
                override_uri: None,
            })?)
        );
        Ok(())
    }
}
