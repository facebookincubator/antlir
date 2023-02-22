/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::convert::Infallible;
use std::net::Ipv6Addr;
use std::net::SocketAddr;
use std::net::SocketAddrV6;
use std::net::TcpListener;
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::Duration;

use anyhow::Context;
use anyhow::Result;
use derivative::Derivative;
use http::Uri;
use hyper::service::make_service_fn;
use hyper::service::service_fn;
use hyper::Body;
use hyper::Client;
use hyper::Request;
use hyper::Response;
use hyper::Server;
use hyperlocal::UnixClientExt;
use tokio::runtime::Runtime;
use tokio::sync::oneshot;

use crate::CompilerContext;

/// [CompilerContext::proxy_socket] provides a UNIX socket connection to the
/// outside world for things like RPM installation. Unfortunately some software
/// (*cough* dnf *cough*) require TCP connections, so we must provide a proxy
/// that listens on TCP and forwards HTTP requests over the UNIX socket.
#[derive(Derivative)]
#[derivative(Debug)]
pub(crate) struct TcpProxy {
    addr: SocketAddr,
    #[derivative(Debug = "ignore")]
    close: Option<oneshot::Sender<()>>,
}

impl TcpProxy {
    /// Start a [TcpProxy] in-process. The server will continue running until
    /// the returned [TcpProxy] is dropped.
    #[tracing::instrument(skip(ctx), ret, err)]
    pub(crate) fn start(ctx: &CompilerContext) -> anyhow::Result<Self> {
        let (addr_tx, addr_rx) = mpsc::channel();
        let (close, close_rx) = oneshot::channel();
        let upstream = ctx.proxy_socket().to_owned();
        std::thread::spawn(move || {
            let rt = Runtime::new().expect("failed to create tokio runtime");
            rt.block_on(serve(upstream, close_rx, addr_tx))
                .expect("server failed to start");
        });
        let addr = addr_rx
            .recv_timeout(Duration::from_millis(250))
            .context("didn't get address from proxy")?;
        Ok(Self {
            addr,
            close: Some(close),
        })
    }

    pub(crate) fn uri_builder(&self) -> http::uri::Builder {
        Uri::builder()
            .scheme("http")
            .authority(self.addr.to_string())
    }
}

#[tracing::instrument(skip_all, fields(uri = req.uri().to_string()), ret, err)]
async fn proxy_request<C>(
    mut req: Request<Body>,
    upstream: PathBuf,
    http_client: Client<C>,
) -> hyper::Result<Response<Body>>
where
    C: hyper::client::connect::Connect + Clone + Send + Sync + 'static,
{
    tracing::trace!(
        "proxying {} to unix://{}:{}",
        req.uri(),
        upstream.display(),
        req.uri().path()
    );
    *req.uri_mut() = hyperlocal::Uri::new(upstream.as_path(), req.uri().path()).into();
    http_client.request(req).await
}

#[tracing::instrument(skip(close, addr_tx), ret, err)]
async fn serve(
    upstream: PathBuf,
    close: oneshot::Receiver<()>,
    addr_tx: mpsc::Sender<SocketAddr>,
) -> Result<()> {
    let addr = SocketAddr::V6(SocketAddrV6::new(Ipv6Addr::LOCALHOST, 0, 0, 0));
    let listener = TcpListener::bind(addr).context("while binding to tcp socket")?;
    addr_tx
        .send(
            listener
                .local_addr()
                .context("while getting local addr from socket")?,
        )
        .context("while sending localaddr")?;

    let make_svc = make_service_fn(move |_conn| {
        let upstream = upstream.clone();
        let http_client = Client::unix();
        async {
            Ok::<_, Infallible>(service_fn(move |req| {
                proxy_request(req, upstream.clone(), http_client.clone())
            }))
        }
    });

    let server = Server::from_tcp(listener)
        .context("while making hyper::Server")?
        .serve(make_svc)
        .with_graceful_shutdown(async {
            let _ = close.await;
            tracing::trace!("shutting down tcp proxy");
        });
    server.await.context("while serving HTTP")
}

impl Drop for TcpProxy {
    fn drop(&mut self) {
        if let Some(close) = self.close.take() {
            close
                .send(())
                .expect("failed to send close message to TcpProxy");
        }
    }
}
