/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::ErrorKind;
use std::pin::Pin;

use async_trait::async_trait;
use bytes::Bytes;
use futures::Stream;
use futures::StreamExt;
use once_cell::sync::Lazy;
use reqwest::Client;
use reqwest::StatusCode;
use slog::debug;
use slog::Logger;
use thiserror::Error;
use url::Url;

use crate::PackageDownloader;
use crate::Result;
use metalos_host_configs::packages::generic::Package;

#[derive(Error, Debug)]
pub enum Error {
    #[error(
        "failure while opening '{url}' for '{package}': {error}",
        url = error.url().map_or_else(|| "<unknown url>".to_string(), |u| u.to_string()),
        package = package.identifier(),
    )]
    Open {
        package: Package,
        error: reqwest::Error,
    },
    #[error(
        "request to '{url}' for '{package}' ended with code {status} (body = '{body}')",
        package = package.identifier(),
    )]
    StatusCode {
        package: Package,
        url: Url,
        status: reqwest::StatusCode,
        body: String,
    },
}

impl From<self::Error> for crate::Error {
    fn from(e: self::Error) -> crate::Error {
        let pkg = match &e {
            self::Error::Open { package, .. } => package,
            self::Error::StatusCode { package, .. } => package,
        };
        crate::Error::Download {
            package: pkg.clone(),
            error: e.into(),
        }
    }
}

#[derive(Clone)]
pub struct HttpsDownloader {
    client: Client,
}

static BASE_URI: Lazy<Url> = Lazy::new(|| {
    let format_uri = {
        #[cfg(facebook)]
        {
            "https://fbpkg.fbinfra.net/fbpkg"
        }
        #[cfg(not(facebook))]
        {
            "https://metalos/package"
        }
    };
    format_uri.parse().unwrap()
});

// https://url.spec.whatwg.org/#path-percent-encode-set
static PATH_ESCAPE_SET: percent_encoding::AsciiSet = percent_encoding::CONTROLS
    .add(b' ')
    .add(b'"')
    .add(b'#')
    .add(b'<')
    .add(b'>')
    .add(b'?')
    .add(b'`')
    .add(b'{')
    .add(b'}');

impl HttpsDownloader {
    pub fn new() -> reqwest::Result<Self> {
        // TODO: it would be nice to restrict to https only, but we use plain
        // http for tests, and https doesn't do much for security compared to
        // something like checking image signatures
        let client = reqwest::Client::builder().user_agent("metalos/1").build()?;
        Ok(Self { client })
    }

    pub fn package_url(&self, pkg: &Package) -> Url {
        match &pkg.override_uri {
            Some(u) => u.clone(),
            None => {
                let mut url = BASE_URI.clone();
                // this cannot fail since we are using a static base url that
                // never changes, so the unit test guarantees that we will never
                // panic
                url.path_segments_mut()
                    .expect("unit test verifies that this cannot fail")
                    .push(
                        &percent_encoding::utf8_percent_encode(&pkg.identifier(), &PATH_ESCAPE_SET)
                            .to_string(),
                    );
                url
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
impl PackageDownloader for &HttpsDownloader {
    type BytesStream = Pin<Box<dyn Stream<Item = std::io::Result<Bytes>> + Send>>;

    async fn open_bytes_stream(&self, log: Logger, pkg: &Package) -> Result<Self::BytesStream> {
        let url = self.package_url(pkg);
        debug!(log, "{:?} -> {}", pkg, url);
        let response = self
            .client
            .get(url.clone())
            .send()
            .await
            .map_err(|e| Error::Open {
                package: pkg.clone(),
                error: e,
            })?;

        if response.status().is_success() {
            Ok(Box::pin(response.bytes_stream().map(|r| {
                r.map_err(|e| std::io::Error::new(ErrorKind::Other, e))
            })))
        } else {
            match response.status() {
                StatusCode::NOT_FOUND => Err(crate::Error::NotFound(pkg.clone())),
                status => Err(Error::StatusCode {
                    package: pkg.clone(),
                    url,
                    status,
                    body: match response.text().await {
                        Ok(body) => body,
                        Err(e) => format!("Failed to ready body: {:?}", e),
                    },
                }
                .into()),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use anyhow::Result;
    use metalos_host_configs::packages::generic::Kind;
    use metalos_host_configs::packages::generic::PackageId;
    use metalos_host_configs::packages::Format;
    use metalos_macros::test;
    use url::Url;

    #[test]
    fn package_url() -> Result<()> {
        let h = HttpsDownloader::new()?;
        let mut pkg = Package {
            name: "metalos.rootfs".to_string(),
            id: PackageId::Uuid("deadbeefdeadbeefdeadbeefdeadbeef".parse().unwrap()),
            override_uri: None,
            format: Format::Sendstream,
            kind: Kind::Rootfs,
        };
        #[cfg(facebook)]
        assert_eq!(
            Url::parse(
                "https://fbpkg.fbinfra.net/fbpkg/metalos.rootfs:deadbeefdeadbeefdeadbeefdeadbeef"
            )
            .unwrap(),
            h.package_url(&pkg)
        );
        #[cfg(not(facebook))]
        assert_eq!(
            Url::parse("https://metalos/package/metalos.rootfs:deadbeefdeadbeefdeadbeefdeadbeef")
                .unwrap(),
            h.package_url(&pkg)
        );
        pkg.override_uri = Some(Url::parse("https://example.com/mypackage").unwrap());
        assert_eq!(
            Url::parse("https://example.com/mypackage").unwrap(),
            h.package_url(&pkg)
        );
        Ok(())
    }

    #[test]
    fn error_format() -> Result<()> {
        let url =
            Url::parse("https://metalos/package/metalos.rootfs:deadbeefdeadbeefdeadbeefdeadbeef")
                .unwrap();

        let e = Error::Open {
            package: Package {
                name: "metalos.rootfs".to_string(),
                id: PackageId::Uuid("deadbeefdeadbeefdeadbeefdeadbeef".parse().unwrap()),
                override_uri: None,
                format: Format::Sendstream,
                kind: Kind::Rootfs,
            },
            error: reqwest::blocking::get(url.clone()).unwrap_err(),
        };
        assert!(
            e.to_string().starts_with(
                "failure while opening \
                'https://metalos/package/metalos.rootfs:deadbeefdeadbeefdeadbeefdeadbeef' \
                for 'metalos.rootfs:deadbeefdeadbeefdeadbeefdeadbeef':"
            ),
            "{}",
            e
        );

        let e = Error::StatusCode {
            package: Package {
                name: "metalos.rootfs".to_string(),
                id: PackageId::Uuid("deadbeefdeadbeefdeadbeefdeadbeef".parse().unwrap()),
                override_uri: None,
                format: Format::Sendstream,
                kind: Kind::Rootfs,
            },
            status: reqwest::StatusCode::IM_A_TEAPOT,
            url,
            body: "body text".into(),
        };
        assert_eq!(
            "request to \
            'https://metalos/package/metalos.rootfs:deadbeefdeadbeefdeadbeefdeadbeef' \
            for 'metalos.rootfs:deadbeefdeadbeefdeadbeefdeadbeef' ended with \
            code 418 I'm a teapot (body = 'body text')",
            e.to_string()
        );
        Ok(())
    }
}
