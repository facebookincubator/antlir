/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![feature(backtrace)]

use structopt::StructOpt;

pub mod send_attribute;
pub mod send_attribute_header;
pub mod send_command;
pub mod send_command_header;
pub mod send_header;
pub mod send_stream;
pub mod send_stream_upgrade_context;
pub mod send_stream_upgrade_options;
pub mod send_stream_upgrade_stats;
pub mod send_version;

#[macro_use]
extern crate maplit;
extern crate num;
#[macro_use]
extern crate num_derive;

pub use crate::send_stream::SendStream;
pub use crate::send_stream_upgrade_options::SendStreamUpgradeOptions;

fn main() -> anyhow::Result<()> {
    let options = SendStreamUpgradeOptions::from_args();
    let mut stream = SendStream::new(options)?;

    stream.upgrade()?;
    Ok(())
}
