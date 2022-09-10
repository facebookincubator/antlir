/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;

use crate::mp::sync::blocking_queue::BlockingQueue;
use crate::mp::threads::worker::Worker;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct WriteWorker {}

unsafe impl Send for WriteWorker {}

impl Worker for WriteWorker {
    fn preserve_source() -> bool {
        // Only the prefetcher can get the source
        false
    }
    fn preserve_destination() -> bool {
        // The writer can get the destination
        true
    }
    fn run_worker(context: SendStreamUpgradeContext) -> anyhow::Result<()> {
        let mut context = context;
        // Detatch the container from the persistence queue
        let sync_container = context
            .ssuc_sync_container
            .as_mut()
            .context("Writing with None container")?;
        let persistence_queue = sync_container
            .take_persistence_queue()
            .context("Writing with None persistence queue")?;

        loop {
            let mut batch_info = match (*persistence_queue).dequeue()? {
                // Got a command batch info
                Some(batch_info) => batch_info,
                // We're walking all of the commands, so this shouldn't happen
                None => anyhow::bail!("Writer received a None batch info"),
            };
            // Take the command out of the batch
            let command = batch_info.remove_first(&mut context)?;
            // Flush it
            command.persist(&mut context)?;
            // Exit if we're done
            if command.is_end() {
                break;
            }
        }

        // On our way out, tally up the stats
        context
            .ssuc_sync_container
            .as_ref()
            .context("Writing with None container")?
            .rollover_stats(&mut context.ssuc_stats)?;
        Ok(())
    }
}
