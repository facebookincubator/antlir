/*
 * Copyright (c) Facebook, Inc. and its affiliates.
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
use maplit::btreemap;

use service_config_generator_if::Dropin;
use service_config_generator_if::Input;
use service_config_generator_if::Output;

fn main() -> Result<()> {
    let mut buf = Vec::new();
    std::io::stdin()
        .read_to_end(&mut buf)
        .context("while reading stdin to buffer")?;
    eprintln!("read {} bytes from stdin", buf.len());
    let input: Input = deserialize(buf).context("while deserializing thrift")?;
    let output = serialize(Output {
        dropin: Some(Dropin {
            environment: btreemap! {
                "GENERATOR_KERNEL_VERSION".into() => input.kernel_version,
            },
        }),
    });
    std::io::stdout()
        .write_all(&output)
        .context("while writing thrift to stdout")?;
    Ok(())
}
