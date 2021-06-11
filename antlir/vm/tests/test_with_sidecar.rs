/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::net::SocketAddrV6;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

#[tokio::test]
async fn sidecar() {
    let sock = SocketAddrV6::new("fd00::1".parse().unwrap(), 8080, 0, 2);
    let mut stream = TcpStream::connect(sock).await.unwrap();

    // Write some data.
    stream.write_all(b"hello world!").await.unwrap();
    let mut buf = [0; 1024];
    stream.read(&mut buf).await.unwrap();
    assert_eq!(
        std::str::from_utf8(&buf)
            .unwrap()
            .trim_end_matches(char::from(0)),
        "hello world!"
    );
}
