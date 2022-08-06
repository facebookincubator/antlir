/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::net::SocketAddrV6;

use anyhow::Error;
use anyhow::Result;
use tokio::io::AsyncReadExt;
use tokio::io::AsyncWriteExt;
use tokio::net::TcpStream;

async fn do_test() -> Result<()> {
    let sock = SocketAddrV6::new("fd00::1".parse()?, 8080, 0, 2);
    let mut stream = TcpStream::connect(sock).await?;

    // Write some data.
    stream.write_all(b"hello world!").await?;
    let mut buf = [0; 1024];
    stream.read(&mut buf).await?;
    assert_eq!(
        std::str::from_utf8(&buf)
            .unwrap()
            .trim_end_matches(char::from(0)),
        "hello world!"
    );
    Ok(())
}

#[tokio::test]
async fn sidecar() -> Result<()> {
    let mut errors = vec![];
    // retry a few times in case the network isn't configured immediately
    for i in 0..3 {
        match do_test().await {
            Ok(_) => return Ok(()),
            Err(e) => {
                eprintln!("error in attempt {}: {:?}", i, e);
                errors.push(format!("{:?}", e));
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    Err(Error::msg(format!("test repeatedly failed: {:#?}", errors)))
}
