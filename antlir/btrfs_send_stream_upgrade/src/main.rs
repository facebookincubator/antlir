/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use btrfs_send_stream_upgrade_lib::upgrade::send_stream::SendStream;
use btrfs_send_stream_upgrade_lib::upgrade::send_stream_upgrade_options::SendStreamUpgradeOptions;
use clap::Parser as _;

fn main() -> anyhow::Result<()> {
    let options = SendStreamUpgradeOptions::parse();
    let mut stream = SendStream::new(options)?;

    stream.upgrade()?;
    Ok(())
}
