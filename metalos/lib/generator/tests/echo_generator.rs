/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Read;
use std::io::Write;

use anyhow::Context;
use anyhow::Result;
use fbthrift::binary_protocol::deserialize;
use fbthrift::binary_protocol::serialize;

fn main() -> Result<()> {
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .context("while reading stdin to buffer")?;
    eprintln!("read {} bytes from stdin", buf.len());
    let input: test_if::Input = deserialize(buf).context("while deserializing thrift")?;
    let output = serialize(test_if::Output { echo: input.hello });
    std::io::stdout()
        .write_all(&output)
        .context("while writing thrift to stdout")?;
    Ok(())
}
