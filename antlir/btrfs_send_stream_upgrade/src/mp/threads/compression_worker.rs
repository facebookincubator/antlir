/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;
use slog::debug;

use crate::mp::sync::blocking_queue::BlockingQueue;
use crate::mp::threads::worker::Worker;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct CompressionWorker {}

unsafe impl Send for CompressionWorker {}

impl Worker for CompressionWorker {
    fn preserve_source() -> bool {
        // Only the prefetcher can get the source
        false
    }
    fn preserve_destination() -> bool {
        // Only the writer can get the destination
        false
    }
    fn run_worker(context: SendStreamUpgradeContext) -> anyhow::Result<()> {
        let mut context = context;
        // Detatch the container from the compression queue
        let sync_container = context
            .ssuc_sync_container
            .as_mut()
            .context("Compressing with None container")?;
        let input_queue = sync_container
            .take_compression_queue()
            .context("Compressing with None compression queue")?;
        let output_queue = sync_container
            .take_persistence_queue()
            .context("Compressing with None persistence queue")?;

        while let Some(mut batch_info) = (*input_queue).dequeue()? {
            let mut command = batch_info.remove_first();
            while !batch_info.is_empty() {
                let next_command = batch_info.remove_first();
                let bytes_appended = command.append(&mut context, &next_command)?;
                anyhow::ensure!(
                    bytes_appended == next_command.get_cached_data_payload_size()?,
                    "Failed to completely append command to batch {} {}",
                    bytes_appended,
                    next_command.get_cached_data_payload_size()?,
                );
            }
            // Attempt compression
            if command.is_compressible() && context.ssuc_options.compression_level != 0 {
                command = match command.compress(&mut context) {
                    Ok(compressed_command) => compressed_command,
                    Err(error) => {
                        match error.downcast_ref::<crate::send_elements::send_attribute::SendAttributeFailedToShrinkPayloadError>() {
                            Some(failed_to_shrink_payload_error) => {
                                // If we failed to shrink the attribute payload,
                                // continue with the old command
                                debug!(context.ssuc_logger,
                                       "Compress Command Failed: {}; proceeding without compression {}",
                                       failed_to_shrink_payload_error,
                                       command);
                                command
                            },
                            // All other errors should just return failures
                            None => anyhow::bail!(error),
                        }
                    }
                };
            }
            // Clean up the command if necessary
            if command.is_dirty() {
                command.flush(&mut context)?;
            }
            // Deposit and continue
            batch_info.repopulate(command)?;
            (*output_queue).enqueue(batch_info)?;
        }

        // On our way out, tally up the stats
        context
            .ssuc_sync_container
            .as_ref()
            .context("Compressing with None container")?
            .rollover_stats(&mut context.ssuc_stats)?;
        Ok(())
    }
}
