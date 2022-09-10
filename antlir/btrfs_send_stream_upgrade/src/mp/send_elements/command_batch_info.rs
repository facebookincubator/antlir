/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::fmt::Display;

use crate::mp::sync::ordered_element::OrderedElement;
use crate::mp::sync::unordered_element::UnorderedElement;
use crate::send_elements::send_command::SendCommand;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

pub struct CommandBatchInfo {
    /// The first id associated with the batch
    cbi_first_id: u64,
    /// The last id associated with the batch
    cbi_last_id: u64,
    /// A list of commands to be batched together
    cbi_command_queue: Vec<SendCommand>,
    /// A running tally of the data payload
    cbi_data_payload_size: usize,
}

impl CommandBatchInfo {
    pub fn new(id: u64, send_command: SendCommand) -> anyhow::Result<Self> {
        let data_payload_size = send_command.get_cached_data_payload_size()?;
        Ok(Self {
            cbi_first_id: id,
            cbi_last_id: id,
            cbi_command_queue: vec![send_command],
            cbi_data_payload_size: data_payload_size,
        })
    }
    pub fn can_append(
        &self,
        other: &Self,
        context: &SendStreamUpgradeContext,
    ) -> anyhow::Result<bool> {
        let length = self.cbi_command_queue.len();
        let last_command = &self.cbi_command_queue[length - 1];
        let other_command = &other.cbi_command_queue[0];
        Ok(last_command.can_append(other_command)
            && (self.cbi_data_payload_size + other.cbi_data_payload_size)
                <= context.ssuc_options.maximum_batched_extent_size
            && (self.cbi_last_id + 1) == other.cbi_first_id)
    }
    pub fn append(&mut self, other: Self) {
        let mut other = other;
        self.cbi_command_queue
            .push(other.cbi_command_queue.remove(0));
        self.cbi_last_id += 1;
        self.cbi_data_payload_size += other.cbi_data_payload_size;
    }
    pub fn remove_first(&mut self) -> SendCommand {
        self.cbi_command_queue.remove(0)
    }
    pub fn is_end(&self) -> bool {
        self.cbi_command_queue[0].is_end()
    }
    pub fn is_appendable(&self) -> bool {
        self.cbi_command_queue[0].is_appendable()
    }
    pub fn is_empty(&self) -> bool {
        self.cbi_command_queue.is_empty()
    }
    pub fn repopulate(&mut self, command: SendCommand) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.cbi_command_queue.is_empty(),
            "Repopulating a non-empty queue"
        );
        self.cbi_command_queue.push(command);
        Ok(())
    }
    fn fmt_internal(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<CommandBatchInfo FirstId={} LastId={} PayloadSize={} CommandQueue={:?}/>",
            self.cbi_first_id, self.cbi_last_id, self.cbi_data_payload_size, self.cbi_command_queue,
        )
    }
}

impl UnorderedElement for CommandBatchInfo {}

unsafe impl Send for CommandBatchInfo {}

impl Debug for CommandBatchInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl Display for CommandBatchInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl OrderedElement for CommandBatchInfo {
    fn get_first_id(&self) -> u64 {
        self.cbi_first_id
    }
    fn get_last_id(&self) -> u64 {
        self.cbi_last_id
    }
    fn is_last_id_shared(&self) -> bool {
        // Verify this while in append and can append
        false
    }
}
