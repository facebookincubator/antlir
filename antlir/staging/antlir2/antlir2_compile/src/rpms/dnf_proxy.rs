/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;
use dnf_conf::DnfConf;
use http::header::HeaderValue;
use http::header::ACCEPT;

use crate::CompilerContext;

#[derive(Debug)]
pub(super) struct DnfProxy {
    dnf_conf: DnfConf,
    /// DnfProxy must hold onto this so that the TcpProxy does not exit until
    /// we're done with dnf actions
    #[allow(dead_code)]
    tcp_proxy: crate::tcp_proxy::TcpProxy,
}

impl DnfProxy {
    /// Start up a thread with a server to process tcp requests from dnf and
    /// forward them over a unix socket, then download it's view of dnf.conf
    #[tracing::instrument(skip(ctx), ret, err)]
    pub(super) fn start(ctx: &CompilerContext) -> anyhow::Result<DnfProxy> {
        let tcp_proxy =
            crate::tcp_proxy::TcpProxy::start(ctx).context("while starting TcpProxy")?;
        let dnf_conf_uri = tcp_proxy
            .uri_builder()
            .path_and_query("/yum/dnf.conf")
            .build()
            .expect("infallible");
        tracing::trace!(uri = dnf_conf_uri.to_string(), "fetching proxied dnf.conf");
        // Yes, blocking requests are bad. However, this (or a more
        // multi-purpose proxy if it emerges in the future) should be the only
        // case where we're doing network io. If that unexpectedly starts being
        // more common in the future, we'll need to make everything properly
        // async.
        let client = reqwest::blocking::Client::new();
        let dnf_conf: DnfConf = client
            .get(dnf_conf_uri.to_string())
            .header(ACCEPT, HeaderValue::from_static("application/json"))
            .send()
            .context("while making request to dnf.conf")?
            .json()
            .context("while downloading and parsing dnf.conf")?;
        tracing::trace!("dnf.conf = {dnf_conf:?}");
        Ok(DnfProxy {
            dnf_conf,
            tcp_proxy,
        })
    }

    pub(super) fn dnf_conf(&self) -> &DnfConf {
        &self.dnf_conf
    }
}
