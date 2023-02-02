/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::convert::Infallible;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use dnf_conf::DnfConf;
use http::header::HOST;
use http::uri::Scheme;
use http::Method;
use http::StatusCode;
use http::Uri;
use hyper::body::Body;
use hyper::service::make_service_fn;
use hyper::service::service_fn;
use hyper::Client;
use hyper::Request;
use hyper::Response;
use hyper::Server;
use hyper_tls::HttpsConnector;
use once_cell::sync::Lazy;
use regex::Regex;
use serde::Deserialize;
use tokio::net::UnixListener;
use tokio_util::codec::BytesCodec;
use tokio_util::codec::FramedRead;
use tracing::span;
use tracing::Level;

mod unix;

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
                        snapshot_base = snapshot_base.trim_matches('/'),
                        rel_key = rel_key.trim_matches('/'),
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

static REPO_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^/yum/(?P<repo>.*)(?P<resource>/(repodata|Packages).*)$"#)
        .expect("will definitely compile")
});
static REPODATA_RE: Lazy<Regex> =
    Lazy::new(|| Regex::new(r#"^/repodata/(?P<artifact>.*)$"#).expect("will definitely compile"));
static PACKAGES_RE: Lazy<Regex> = Lazy::new(|| {
    Regex::new(r#"^/Packages/(?P<id>[a-f0-9]{64})/(?P<name>.*\.rpm)$"#)
        .expect("will definitely compile")
});

#[derive(Debug, thiserror::Error)]
#[error("{status}: {msg}")]
struct ReplyError {
    status: StatusCode,
    msg: String,
}

impl ReplyError {
    fn new(status: StatusCode, msg: impl std::fmt::Display) -> Self {
        Self {
            status,
            msg: msg.to_string(),
        }
    }

    fn not_found(msg: impl std::fmt::Display) -> Self {
        Self::new(StatusCode::NOT_FOUND, msg)
    }
}

impl From<ReplyError> for Response<Body> {
    fn from(mut re: ReplyError) -> Self {
        re.msg.push('\n');
        Response::builder()
            .status(re.status)
            .body(re.msg.into())
            .expect("infallible")
    }
}

#[tracing::instrument(skip_all, fields(path=req.uri().path()))]
async fn service_request<C>(
    req: Request<Body>,
    rpm_repos: Arc<HashMap<String, RpmRepo>>,
    http_client: Arc<Client<C>>,
) -> Result<Response<Body>, Infallible>
where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    let span = span!(Level::TRACE, "reply", path = req.uri().path());
    let _guard = span.enter();
    match reply(req, rpm_repos, http_client).await {
        Ok(resp) => Ok(resp),
        Err(err) => {
            tracing::error!(status = err.status.to_string(), "{}", err.msg);
            Ok(err.into())
        }
    }
}

async fn serve_static_file(path: &Path) -> Result<Response<Body>, ReplyError> {
    match tokio::fs::File::open(&path).await {
        Ok(f) => {
            let stream = FramedRead::new(f, BytesCodec::new());
            Ok(Response::builder()
                .body(Body::wrap_stream(stream))
                .expect("infallible"))
        }
        Err(e) => match e.kind() {
            std::io::ErrorKind::NotFound => Err(ReplyError::not_found(format!(
                "'{}' not found",
                path.display()
            ))),
            _ => Err(ReplyError::new(
                StatusCode::INTERNAL_SERVER_ERROR,
                format!("'{}': {e}", path.display()),
            )),
        },
    }
}

async fn reply<C>(
    req: Request<Body>,
    rpm_repos: Arc<HashMap<String, RpmRepo>>,
    http_client: Arc<Client<C>>,
) -> Result<Response<Body>, ReplyError>
where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    if req.method() != Method::GET {
        return Err(ReplyError {
            status: StatusCode::METHOD_NOT_ALLOWED,
            msg: format!("'{}' is not allowed", req.method()),
        });
    }

    let path = req.uri().path();
    if let Some(cap) = REPO_RE.captures(path) {
        let repo_id = cap.name("repo").expect("'repo' must exist").as_str();
        let repo = match rpm_repos.get(repo_id) {
            Some(repo) => repo,
            None => {
                return Err(ReplyError::not_found(format!(
                    "repo '{repo_id} does not exist"
                )));
            }
        };
        let resource = cap
            .name("resource")
            .expect("'resource' must exist")
            .as_str();
        if let Some(cap) = REPODATA_RE.captures(resource) {
            let path = repo.repodata_dir.join(
                cap.name("artifact")
                    .expect("'artifact' must exist")
                    .as_str(),
            );
            serve_static_file(&path).await
        } else if let Some(cap) = PACKAGES_RE.captures(resource) {
            let id = cap.name("id").expect("'id' must exist").as_str();
            let name = cap.name("name").expect("'name' must exist").as_str();
            if let Some(offline_dir) = repo
                .offline_dir
                .as_deref()
                .and_then(|path| if path.exists() { Some(path) } else { None })
            {
                serve_static_file(&offline_dir.join("Packages").join(id).join(name)).await
            } else {
                let rel_key = format!("antlir_fast_snapshot_{id}.rpm");
                let url = repo.urlgen.urlgen(&rel_key);
                tracing::trace!("{repo_id}/{rel_key}/{name} -> {url}");
                match http_client.get(url.clone()).await {
                    Ok(resp) => {
                        if !resp.status().is_success() {
                            tracing::error!(
                                url = url.to_string(),
                                status = resp.status().to_string(),
                                "upstream failed"
                            );
                        }
                        Ok(resp)
                    }
                    Err(e) => Err(ReplyError::new(StatusCode::BAD_GATEWAY, e)),
                }
            }
        } else {
            Err(ReplyError::not_found(format!(
                "repo {repo_id} exists but '{resource}' does not match any known resources"
            )))
        }
    } else if path == "/yum/dnf.conf" {
        // find the hostname that this was called with so we can put the right
        // thing in the dnf.conf for absolute urls
        let authority = req
            .headers()
            .get(HOST)
            .expect("HOST header is always present")
            .to_str()
            .expect("HOST header must be valid string");

        let mut builder = DnfConf::builder();
        for (id, _) in rpm_repos.iter() {
            let uri = Uri::builder()
                .scheme(
                    req.uri()
                        .scheme()
                        .cloned()
                        .unwrap_or_else(|| Scheme::HTTP.clone()),
                )
                .authority(authority)
                .path_and_query(format!("/yum/{}", id,))
                .build()
                .expect("all parts have already been validated");
            builder.add_repo(id.clone(), uri);
        }
        Ok(Response::builder()
            .body(builder.build().to_string().into())
            .expect("infallible"))
    } else {
        Err(ReplyError::not_found("does not match any routes"))
    }
}

#[tracing::instrument(ret, err)]
pub async fn serve(cfg: Config) -> Result<()> {
    let rpm_repos: Arc<HashMap<_, _>> = Arc::new(
        cfg.rpm_repos
            .into_iter()
            .map(|(id, repo)| (id, repo))
            .collect(),
    );

    let listener = UnixListener::bind(&cfg.bind).context("while binding unix socket")?;
    let incoming = crate::unix::UnixIncoming(listener);
    tracing::info!("bound to {:?}", &cfg.bind);

    let make_svc = make_service_fn(move |_conn| {
        let rpm_repos = rpm_repos.clone();
        let http_client = Arc::new(Client::builder().build::<_, Body>(HttpsConnector::new()));
        async {
            Ok::<_, Infallible>(service_fn(move |req| {
                service_request(req, rpm_repos.clone(), http_client.clone())
            }))
        }
    });

    let server = Server::builder(incoming)
        .serve(make_svc)
        .with_graceful_shutdown(async move {
            tokio::signal::ctrl_c()
                .await
                .expect("failed to listen to shutdown signal");
        });
    let result = server.await;

    std::fs::remove_file(&cfg.bind).context("while removing socket")?;
    result.context("while serving HTTP")
}
