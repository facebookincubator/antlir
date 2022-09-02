/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use crate::mp::threads::batcher_worker::BatcherWorker;
use crate::mp::threads::command_construction_worker::CommandConstructionWorker;
use crate::mp::threads::compression_worker::CompressionWorker;
use crate::mp::threads::prefetch_worker::PrefetchWorker;
use crate::mp::threads::read_worker::ReadWorker;
use crate::mp::threads::worker::Worker;
use crate::mp::threads::write_worker::WriteWorker;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct Coordinator<'a> {
    /// The base context
    c_context: Option<SendStreamUpgradeContext<'a>>,
}

impl<'a> Coordinator<'a> {
    pub fn new(context: Option<SendStreamUpgradeContext<'a>>) -> anyhow::Result<Self> {
        Ok(Self { c_context: context })
    }
    pub fn run(&mut self) -> anyhow::Result<()> {
        match self.c_context {
            Some(ref mut context) => {
                // Be sure to flush the context before starting any IO on it
                context.flush()?;
                // Set up the sync container for mp accesses
                context.setup_sync_container()?;
            }
            None => anyhow::bail!("None context in coordinator"),
        }
        let context = match self.c_context {
            Some(ref mut context) => context,
            None => anyhow::bail!("None context in coordinator"),
        };
        {
            let _batcher = BatcherWorker::new("Batcher worker".to_string(), context)?;
            let _command_constructor =
                CommandConstructionWorker::new("Command construction worker".to_string(), context)?;
            let _compressor = CompressionWorker::new("Compression worker".to_string(), context)?;
            let _prefetcher = PrefetchWorker::new("Prefetch worker".to_string(), context)?;
            let _reader = ReadWorker::new("Read worker".to_string(), context)?;
            let _writer = WriteWorker::new("Write worker".to_string(), context)?;
        }
        Ok(())
    }
    pub fn take_context(&mut self) -> Option<SendStreamUpgradeContext<'a>> {
        self.c_context.take()
    }
}
