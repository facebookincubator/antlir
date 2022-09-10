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
use crate::send_elements::send_command::SendCommand;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct CommandConstructionWorker {}

unsafe impl Send for CommandConstructionWorker {}

impl Worker for CommandConstructionWorker {
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
        // Detatch the container from the command construction queue
        let sync_container = context
            .ssuc_sync_container
            .as_mut()
            .context("Constructing commands with None container")?;
        let input_queue = sync_container
            .take_command_construction_queue()
            .context("Constructing commands with None command construction queue")?;
        let output_queue = sync_container
            .take_batcher_queue()
            .context("Constructing commands with None batcher queue")?;

        // Loop around generating commands
        while let Some(command_info) = (*input_queue).dequeue()? {
            let (id, header, offset) = command_info.split();
            // Seek to the correct location
            context.set_read_offset(offset);
            let mut command = SendCommand::new_from_header(&mut context, header)?;

            // Run upgrade on the command
            if command.is_upgradeable(&context)? {
                command = command.upgrade(&mut context)?;
            } else {
                command.fake_an_upgrade(&context)?;
            }

            // Pack the command into a command batch info for further processing
            let command_batch_info = CommandBatchInfo::new(id, command)?;
            // Send this off to the batcher
            (*output_queue).enqueue(command_batch_info)?;
        }

        // On our way out, tally up the stats
        context
            .ssuc_sync_container
            .as_ref()
            .context("Constructing commands with None container")?
            .rollover_stats(&mut context.ssuc_stats)?;
        Ok(())
    }
}
