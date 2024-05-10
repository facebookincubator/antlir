/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::convert::Infallible;
use std::fmt::Debug;
use std::future::Future;
use std::path::Path;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::Context;
use anyhow::Result;
use derivative::Derivative;
use dnf_conf::DnfConf;
use http::header::HeaderValue;
use http::header::ACCEPT;
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
use tokio::sync::oneshot;
use tokio_util::codec::BytesCodec;
use tokio_util::codec::FramedRead;
use tracing::span;
use tracing::Level;
use uuid::Uuid;

mod unix;

#[derive(Derivative)]
#[derivative(Debug)]
pub struct Config {
    rpm_repos: HashMap<String, RpmRepo>,
    bind: Bind,
    #[derivative(Debug = "ignore")]
    graceful_shutdown: Pin<Box<dyn Future<Output = ()> + Sync + Send>>,
}

impl Config {
    pub fn new(
        rpm_repos: HashMap<String, RpmRepo>,
        bind: Bind,
        graceful_shutdown: Option<Pin<Box<dyn Future<Output = ()> + Sync + Send>>>,
    ) -> Self {
        Self {
            rpm_repos,
            bind,
            graceful_shutdown: graceful_shutdown.unwrap_or_else(|| {
                Box::pin(async move {
                    tokio::signal::ctrl_c()
                        .await
                        .expect("failed to listen to shutdown signal");
                })
            }),
        }
    }
}

pub enum Bind {
    /// Bind to a pre-known path
    Path(PathBuf),
    /// Bind to a temporary file and send it on this channel once bound
    Dynamic(oneshot::Sender<PathBuf>),
}

impl Debug for Bind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Path(p) => f.debug_tuple("Path").field(&p).finish(),
            Self::Dynamic(_) => f.debug_tuple("Dynamic").finish(),
        }
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
    Regex::new(r#"^/Packages/(?P<id>[a-f0-9]+)/(?P<name>.*\.rpm)$"#)
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

static JSON_MIME: HeaderValue = HeaderValue::from_static("application/json");

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

        let dnf_conf = builder.build();

        if req.headers().get(ACCEPT).map_or(false, |v| v == JSON_MIME) {
            Ok(Response::builder()
                .body(
                    serde_json::to_vec(&dnf_conf)
                        .expect("this is valid json")
                        .into(),
                )
                .expect("infallible"))
        } else {
            Ok(Response::builder()
                .body(dnf_conf.to_string().into())
                .expect("infallible"))
        }
    } else {
        Err(ReplyError::not_found("does not match any routes"))
    }
}

#[tracing::instrument(ret, err)]
pub async fn serve(cfg: Config) -> Result<()> {
    let rpm_repos: Arc<HashMap<_, _>> = Arc::new(cfg.rpm_repos.into_iter().collect());

    let path = match &cfg.bind {
        Bind::Path(p) => p.to_path_buf(),
        Bind::Dynamic(_) => Path::new("/tmp").join(format!("antlir_{}", Uuid::new_v4())),
    };

    let listener = UnixListener::bind(&path).context("while binding unix socket")?;
    let incoming = crate::unix::UnixIncoming(listener);
    tracing::info!("bound to {:?}", &cfg.bind);
    if let Bind::Dynamic(tx) = cfg.bind {
        tracing::debug!(path = path.display().to_string(), "bound to dynamic path");
        tx.send(path.clone())
            .map_err(|_| anyhow::Error::msg("dynamic socket receiver is closed"))?;
    }

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
        .with_graceful_shutdown(cfg.graceful_shutdown);
    let result = server.await;

    tracing::debug!("unlinking socket");
    std::fs::remove_file(&path).context("while removing socket")?;
    result.context("while serving HTTP")
}
