/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;

use crate::mp::send_elements::command_info::CommandInfo;
use crate::mp::sync::blocking_queue::BlockingQueue;
use crate::mp::threads::worker::Worker;
use crate::send_elements::send_command_header::SendCommandHeader;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct ReadWorker {}

unsafe impl Send for ReadWorker {}

impl Worker for ReadWorker {
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
            .context("Reading with None container")?;
        let queue = sync_container
            .take_command_construction_queue()
            .context("Reading with None command construction queue")?;

        let mut command_id: u64 = 0;
        loop {
            // Read in the header in a loop
            let header = SendCommandHeader::new(&mut context)?;
            let payload_size = header.get_command_payload_size()?;
            let is_end = header.is_command_end();
            let start_address = context.get_read_offset();
            // Generate the new command information
            let command_info = CommandInfo::new(command_id, header, start_address)?;
            // Enqueue into the command construction queue
            (*queue).enqueue(command_info)?;
            // Are we now at the end?
            if is_end {
                break;
            }
            // Skip ahead to the next command
            context.adjust_read_offset(payload_size)?;
            command_id += 1;
        }

        // On our way out, tally up the stats
        context
            .ssuc_sync_container
            .as_ref()
            .context("Reading with None container")?
            .rollover_stats(&mut context.ssuc_stats)?;
        Ok(())
    }
}
