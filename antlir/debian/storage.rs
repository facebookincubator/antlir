/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use core::num::NonZeroU32;

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use governor::clock::DefaultClock;
use governor::state::direct::NotKeyed;
use governor::state::InMemoryState;
use http::StatusCode as HttpStatusCode;
use manifold_client::cpp_client::ManifoldCppClient;
use manifold_client::read::ReadRequestOptionsBuilder;
use manifold_client::write::WriteRequestOptionsBuilder;
use manifold_client::ManifoldClient;
use reqwest::StatusCode;
use sha2::Digest;
use sha2::Sha256;
use slog::error;
use slog::info;
use slog::o;
use tokio_retry::strategy::ExponentialBackoff;
use tokio_retry::RetryIf;

struct BlobError {
    url: reqwest::Url,
    is_retryable: bool,
    status: Option<StatusCode>,
    req_err: anyhow::Error,
}

impl BlobError {
    fn new(
        url: reqwest::Url,
        is_retryable: bool,
        status: Option<StatusCode>,
        req_err: anyhow::Error,
    ) -> Self {
        BlobError {
            url,
            is_retryable,
            status,
            req_err,
        }
    }
}

async fn fetch_blob(
    url: reqwest::Url,
    cli: &reqwest::Client,
    logger: slog::Logger,
) -> std::result::Result<Bytes, BlobError> {
    info!(logger, "fetching");
    match cli.get(url.clone()).send().await {
        Ok(resp) => {
            if resp.status().is_success() {
                resp.bytes()
                    .await
                    .map_err(|e| BlobError::new(url.clone(), true, None, anyhow::Error::from(e)))
            } else {
                Err(BlobError::new(
                    url.clone(),
                    !resp.status().is_client_error()
                        || resp.status() == HttpStatusCode::TOO_MANY_REQUESTS,
                    Some(resp.status()),
                    anyhow!("http error: {} url: {}", resp.status(), url.clone()),
                ))
            }
        }
        Err(e) => Err(BlobError::new(
            url.clone(),
            true,
            None,
            anyhow::Error::from(e),
        )),
    }
}

pub async fn get_blob(
    url: reqwest::Url,
    cli: &reqwest::Client,
    retry_num: usize,
    logger: slog::Logger,
) -> Result<Bytes> {
    let logger = logger.new(o!("url" => url.clone().to_string()));
    match RetryIf::spawn(
        ExponentialBackoff::from_millis(100).take(retry_num),
        || fetch_blob(url.clone(), cli, logger.clone()),
        |e: &BlobError| e.is_retryable,
    )
    .await
    {
        Ok(bytes) => Ok(bytes),
        Err(e) => {
            error!(logger, "failed to fetch"; "retry_count" => retry_num, "http_error_code" => format!("{:?}", e.status));
            Err(e
                .req_err
                .context(format!("cannot get blob from url: {}", e.url)))
        }
    }
}

pub enum Blob {
    Url(reqwest::Url),
    Blob(Bytes),
}
pub fn get_sha2_hash(input: impl AsRef<[u8]>) -> String {
    let mut sha2_hasher = Sha256::new();
    sha2_hasher.update(input);
    hex::encode(sha2_hasher.finalize())
}

pub struct DownloadDetails {
    pub content: Blob,
    pub key: String,
    pub name: String,
    pub version: String,
}

pub trait StoreFormat {
    fn store_format(&self) -> Result<DownloadDetails>;
}

#[async_trait]
pub trait PackageBackend: Send + Sync {
    async fn key_exists(&self, key: &str) -> Result<bool>;
    async fn put(&self, blob: Bytes, key: &str) -> Result<()>;
    async fn get(&self, key: &str) -> Result<Bytes>;
}

pub struct RateLimitedPackageBackend<T: PackageBackend + Sync + Send> {
    read_qps_limit: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    write_qps_limit: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    write_throughput_limit: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
    backend: T,
}
impl<T: PackageBackend + Sync + Send> RateLimitedPackageBackend<T> {
    pub fn new(
        read_qps_limit: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
        write_qps_limit: Option<governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>>,
        write_throughput_limit: Option<
            governor::RateLimiter<NotKeyed, InMemoryState, DefaultClock>,
        >,
        backend: T,
    ) -> Self {
        RateLimitedPackageBackend {
            read_qps_limit,
            write_qps_limit,
            write_throughput_limit,
            backend,
        }
    }
}
#[async_trait]
impl<B> PackageBackend for &RateLimitedPackageBackend<B>
where
    B: PackageBackend + Sync + Send,
{
    async fn key_exists(&self, key: &str) -> Result<bool> {
        if let Some(limiter) = &self.read_qps_limit {
            limiter.until_ready().await;
        }
        Ok(self.backend.key_exists(key).await?)
    }
    async fn put(&self, blob: Bytes, key: &str) -> Result<()> {
        if let Some(limiter) = &self.write_qps_limit {
            limiter.until_ready().await;
        }
        if let Some(limiter) = &self.write_throughput_limit {
            let size = NonZeroU32::new(blob.len().try_into()?).context("empty file")?;
            match limiter.until_n_ready(size).await {
                Ok(()) => (),
                Err(e) => {
                    limiter
                        .until_n_ready(NonZeroU32::new(e.0).expect("limit checked before"))
                        .await?
                }
            }
        }
        Ok(self.backend.put(blob, key).await?)
    }
    async fn get(&self, key: &str) -> Result<Bytes> {
        if let Some(limiter) = &self.read_qps_limit {
            limiter.until_ready().await;
        }
        Ok(self.get(key).await?)
    }
}

pub async fn upload<S: StoreFormat, T: PackageBackend>(
    item: S,
    backend: T,
    cli: &reqwest::Client,
    logger: slog::Logger,
) -> Result<()> {
    let pkg = item.store_format()?;
    if backend.key_exists(&pkg.key).await? {
        info!(logger, ""; "package_exists" => true, "key" => pkg.key);
        return Ok(());
    }
    let logger = logger.new(o!("package_exists" => false));
    let blob = match pkg.content {
        Blob::Url(req_url) => get_blob(req_url, cli, 4, logger.new(o!("file" => "deb"))).await?,
        Blob::Blob(byt) => byt,
    };
    backend.put(blob, &pkg.key).await
}

#[async_trait]
impl PackageBackend for ManifoldCppClient {
    async fn key_exists(&self, key: &str) -> Result<bool> {
        let key = format!("flat/{}", key);
        Ok(self.exists(&key).await?)
    }
    async fn put(&self, blob: Bytes, key: &str) -> Result<()> {
        let key = format!("flat/{}", key);
        let write_req_opts = WriteRequestOptionsBuilder::default()
            .build()
            .map_err(Error::msg)?;
        let mut write_req = self.create_write_request(&write_req_opts)?;
        write_req.write(&key, blob.into()).await?;
        Ok(())
    }
    async fn get(&self, key: &str) -> Result<Bytes> {
        let key = format!("flat/{}", key);
        let read_req_opts = ReadRequestOptionsBuilder::default()
            .build()
            .map_err(Error::msg)?;
        let mut read_req = self.create_read_request(&read_req_opts)?;
        Ok(read_req.read(&key).await?.payload.payload)
    }
}
