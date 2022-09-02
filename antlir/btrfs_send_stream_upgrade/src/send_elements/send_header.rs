/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Display;
use std::str;

use slog::info;

use crate::send_elements::send_version::SendVersion;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

const BTRFS_MAGIC: &str = "btrfs-stream\0";

pub struct SendHeader {
    /// The magic value embedded in the header
    sh_magic: String,
    /// The version of the send stream
    sh_version: SendVersion,
}

impl Display for SendHeader {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<SendHeader Magic={}, Version={:?}/>",
            self.sh_magic, self.sh_version
        )
    }
}

impl SendHeader {
    pub fn new(context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        let mut magic_bytes = [0; BTRFS_MAGIC.len()];
        context.read_exact(&mut magic_bytes)?;
        let magic = str::from_utf8(&magic_bytes)?;
        anyhow::ensure!(
            magic.eq(BTRFS_MAGIC),
            "Found magic {} was expecting {}",
            magic,
            BTRFS_MAGIC
        );
        let version_value = context.read32()?;
        let version = SendStreamUpgradeContext::value_to_version(version_value)?;
        let header = SendHeader {
            sh_magic: String::from(magic),
            sh_version: version,
        };
        info!(context.ssuc_logger, "New Header={}", header);
        Ok(header)
    }

    pub fn persist_header(context: &mut SendStreamUpgradeContext) -> anyhow::Result<()> {
        context.trace_stats();
        let magic_bytes = BTRFS_MAGIC.as_bytes();
        info!(
            context.ssuc_logger,
            "Writing magic of {} with {:02X?} bytes", BTRFS_MAGIC, magic_bytes
        );
        context.write(magic_bytes, magic_bytes.len())?;
        context.trace_stats();
        let version = context.get_destination_version()?;
        let version_value = SendStreamUpgradeContext::version_to_value(version);
        info!(context.ssuc_logger, "Writing version of {}", version_value);
        context.write32(version_value)?;
        Ok(())
    }

    pub fn get_version(&self) -> SendVersion {
        self.sh_version
    }

    pub fn get_size() -> usize {
        BTRFS_MAGIC.len() + std::mem::size_of::<u32>()
    }
}
