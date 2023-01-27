/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use dnf_conf::DnfConf;
use http::StatusCode;
use hyper::body::Body;
use hyper::Response;
use hyper::Uri;
use serde::Deserialize;
use tokio::net::UnixListener;
use tokio_stream::wrappers::UnixListenerStream;
use tokio_util::codec::BytesCodec;
use tokio_util::codec::FramedRead;
use warp::host::Authority;
use warp::reject::Reject;
use warp::Filter;
use warp::Rejection;

#[derive(Debug)]
pub struct Config {
    rpm_repos: HashMap<String, RpmRepo>,
    bind: PathBuf,
}

impl Config {
    pub fn new(rpm_repos: HashMap<String, RpmRepo>, bind: PathBuf) -> Self {
        Self { rpm_repos, bind }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub enum UrlGen {
    Offline,
    #[cfg(facebook)]
    Manifold {
        api_key: String,
        bucket: String,
        snapshot_base: String,
    },
}

impl UrlGen {
    fn urlgen(&self, rel_key: &str) -> Uri {
        match self {
            Self::Offline => panic!("offline repos have no url capabilities"),
            #[cfg(facebook)]
            Self::Manifold {
                api_key,
                bucket,
                snapshot_base
            } => {
                Uri::builder()
                    .scheme("https")
                    .authority("manifold.facebook.net").
                    path_and_query(&format!(
                        "/v0/read/{snapshot_base}/{rel_key}?bucketName={bucket}&apiKey={apikey}&timeoutMsec={timeout_ms}&withPayload=1",
                        snapshot_base = snapshot_base,
                        bucket = bucket,
                        apikey = api_key,
                        timeout_ms = 30_000,
                    ))
                    .build().expect("valid url")
            }
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct RpmRepo {
    repodata_dir: PathBuf,
    offline_dir: Option<PathBuf>,
    urlgen: UrlGen,
}

async fn serve_small_static_file(path: PathBuf) -> std::result::Result<Vec<u8>, Rejection> {
    match tokio::fs::read(&path).await {
        Ok(bytes) => Ok(bytes),
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => Err(warp::reject::not_found()),
            _ => Err(warp::reject::custom(AnyhowRejection(e.into()))),
        },
    }
}

#[derive(Debug)]
struct AnyhowRejection(anyhow::Error);

impl Reject for AnyhowRejection {}

#[tracing::instrument(ret, err)]
pub async fn serve(cfg: Config) -> Result<()> {
    let rpm_repos: Arc<HashMap<_, _>> = Arc::new(
        cfg.rpm_repos
            .into_iter()
            .map(|(id, repo)| (id, Arc::new(repo)))
            .collect(),
    );

    let alive = warp::path("_alive").map(|| "OK\n");
    let rpm_repos2 = rpm_repos.clone();
    let repodata = warp::get()
        .and(warp::path("yum"))
        .and(warp::path::param())
        .and(warp::path("repodata"))
        .and(warp::path::param())
        .and(warp::path::end())
        .and_then(move |repo_id: String, artifact: String| {
            futures::future::ready(match rpm_repos2.get(&repo_id) {
                Some(repo) => Ok(repo.repodata_dir.join(artifact)),
                None => Err(warp::reject::not_found()),
            })
        })
        .and_then(serve_small_static_file);

    let http_client = Arc::new(
        hyper::Client::builder().build::<_, hyper::Body>(hyper_tls::HttpsConnector::new()),
    );

    let rpm_repos2 = rpm_repos.clone();
    let package_blob = warp::get()
        .and(warp::path("yum"))
        .and(warp::path::param())
        .and(warp::path("Packages"))
        .and(warp::path::param())
        .and(warp::path::param())
        .and(warp::path::end())
        .and_then(move |repo_id: String, id: String, name: String| {
            let rpm_repos = rpm_repos2.clone();
            let http_client = http_client.clone();
            async move {
                let repo_id = percent_encoding::percent_decode_str(&repo_id)
                    .decode_utf8()
                    .context("invalid url-encoded repo_id")
                    .map_err(|e| warp::reject::custom(AnyhowRejection(e)))?;
                let repo = rpm_repos.get(repo_id.as_ref()).cloned();
                let name = percent_encoding::percent_decode_str(&name)
                    .decode_utf8()
                    .context("invalid url-encoded name")
                    .map_err(|e| warp::reject::custom(AnyhowRejection(e)))?;
                let repo = match repo {
                    Some(r) => r,
                    None => return Err(warp::reject::not_found()),
                };
                if let Some(offline_dir) = &repo.offline_dir {
                    let offline_path = offline_dir.join("Packages").join(&id).join(name.as_ref());
                    match tokio::fs::File::open(&offline_path).await {
                        Ok(file) => {
                            tracing::trace!("{} exists, serving from disk", offline_path.display());
                            let stream = FramedRead::new(file, BytesCodec::new());
                            return Ok(Response::builder()
                                .status(StatusCode::OK)
                                .body(Body::wrap_stream(stream))
                                .expect("always valid"));
                        }
                        Err(e) => match e.kind() {
                            std::io::ErrorKind::NotFound => {
                                tracing::trace!(
                                    "{} does not exist, trying remote",
                                    offline_path.display()
                                );
                            }
                            _ => return Err(warp::reject::custom(AnyhowRejection(e.into()))),
                        },
                    }
                }
                let rel_key = format!("antlir_fast_snapshot_{id}.rpm");
                let url = repo.urlgen.urlgen(&rel_key);
                tracing::trace!("{repo_id}/{rel_key}/{name} -> {url}");
                match http_client.get(url).await {
                    Ok(resp) => Ok::<_, warp::reject::Rejection>(resp),
                    Err(e) => Ok(Response::builder()
                        .status(StatusCode::BAD_GATEWAY)
                        .body(Body::from(e.to_string()))
                        .expect("always valid")),
                }
            }
        });

    let dnf_conf = warp::get()
        .and(warp::path!("yum" / "dnf.conf"))
        .and(warp::host::optional())
        .map(move |authority: Option<Authority>| {
            let authority = authority.unwrap_or_else(|| Authority::from_static("localhost"));
            let mut builder = DnfConf::builder();
            for (id, _) in rpm_repos.iter() {
                let uri = Uri::builder()
                    .scheme("http")
                    .authority(authority.clone())
                    .path_and_query(format!("/yum/{}", id))
                    .build()
                    .expect("all parts have already been validated");
                builder.add_repo(
                    id.clone(),
                    url::Url::parse(&uri.to_string())
                        .expect("inefficient conversion, but it will always work"),
                );
            }
            builder.build().to_string()
        });

    let routes = alive
        .or(repodata)
        .or(package_blob)
        .or(dnf_conf)
        .with(warp::trace::request());

    let listener = UnixListener::bind(&cfg.bind).context("while binding unix socket")?;
    let incoming = UnixListenerStream::new(listener);
    tracing::info!("bound to {:?}", &cfg.bind);

    warp::serve(routes)
        .serve_incoming_with_graceful_shutdown(incoming, async move {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen to shutdown signal");
        })
        .await;
    std::fs::remove_file(&cfg.bind).context("while removing socket")?;
    Ok(())
}
