/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use hyper::server::accept::Accept;

pub(crate) struct UnixIncoming(pub(crate) tokio::net::UnixListener);

impl Accept for UnixIncoming {
    type Conn = tokio::net::UnixStream;
    type Error = std::io::Error;

    fn poll_accept(
        self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Result<Self::Conn, Self::Error>>> {
        self.0
            .poll_accept(cx)
            .map_ok(|(stream, _addr)| stream)
            .map(Some)
    }
}
