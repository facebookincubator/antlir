/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;

use crate::mp::send_elements::command_batch_info::CommandBatchInfo;
use crate::mp::sync::blocking_queue::BlockingQueue;
use crate::mp::threads::worker::Worker;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct BatcherWorker {}

unsafe impl Send for BatcherWorker {}

impl Worker for BatcherWorker {
    fn preserve_source() -> bool {
        // Only the prefetcher can get the source
        false
    }
    fn preserve_destination() -> bool {
        // Only the writer can get the source
        false
    }
    fn run_worker(context: SendStreamUpgradeContext) -> anyhow::Result<()> {
        let mut context = context;
        // Detatch the container from the batcher queue
        let sync_container = context
            .ssuc_sync_container
            .as_mut()
            .context("Batching with None container")?;
        let input_queue = sync_container
            .take_batcher_queue()
            .context("Batching with None batcher queue")?;
        let compression_queue = sync_container
            .take_compression_queue()
            .context("Batching with None compression queue")?;
        let persistence_queue = sync_container
            .take_persistence_queue()
            .context("Batching with None persistence queue")?;

        let mut previous_batch_info_option: Option<CommandBatchInfo> = None;

        loop {
            let current_batch_info = match (*input_queue).dequeue()? {
                // Got a command batch info
                Some(batch_info) => batch_info,
                // We're walking all of the commands, so this shouldn't happen
                None => anyhow::bail!("Batcher received a None batch info"),
            };
            match previous_batch_info_option {
                // See if we can append the new one to the old one
                Some(mut previous_batch_info) => {
                    if previous_batch_info.can_append(&current_batch_info, &context)? {
                        previous_batch_info.append(current_batch_info);
                        // Reset the previous batch info option
                        // This is to make rustc happy -- declaring
                        // previous batch info to be mut above resulted in
                        // the batch info being moved out of the option
                        // Now we must restore it
                        previous_batch_info_option = Some(previous_batch_info);
                        // Try to get the next command
                        continue;
                    } else {
                        // Could not append the commands
                        // Send the previous command off to the compression
                        // queue
                        (*compression_queue).enqueue(previous_batch_info)?;
                        previous_batch_info_option = None;
                    }
                }
                None => (),
            }
            let is_end = current_batch_info.is_end();
            // If the command isn't appendable, send it off to the persistence
            // queue
            if !current_batch_info.is_appendable() {
                (*persistence_queue).enqueue(current_batch_info)?;
            } else {
                // Otherwise, let's try to add to this batch
                previous_batch_info_option = Some(current_batch_info);
            }
            // Bail if we're done
            if is_end {
                break;
            }
        }

        // On our way out, tally up the stats
        context
            .ssuc_sync_container
            .as_ref()
            .context("Batching with None container")?
            .rollover_stats(&mut context.ssuc_stats)?;
        Ok(())
    }
}
