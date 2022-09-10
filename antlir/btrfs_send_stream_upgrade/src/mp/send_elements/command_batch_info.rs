/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::fmt::Display;
use std::sync::Arc;

use crate::mp::sync::ordered_element::OrderedElement;
use crate::mp::sync::unordered_element::UnorderedElement;
use crate::send_elements::send_command::SendCommand;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

struct SendCommandPointer {
    /// The command being referenced
    scp_command: Arc<SendCommand>,
    /// The start offset within the command's data payload
    scp_data_payload_start_offset: usize,
    /// The end offset within the command's data payload
    scp_data_payload_end_offset: usize,
}

pub struct CommandBatchInfo {
    /// The first id associated with the batch
    cbi_first_id: u64,
    /// The last id associated with the batch
    cbi_last_id: u64,
    /// Whether the last id is shared or not
    cbi_last_id_shared: bool,
    /// A list of commands to be batched together
    cbi_command_queue: Vec<SendCommandPointer>,
    /// A running tally of the data payload
    cbi_data_payload_size: usize,
}

impl CommandBatchInfo {
    pub fn new(id: u64, send_command: SendCommand) -> anyhow::Result<Self> {
        let data_payload_size = send_command.get_cached_data_payload_size()?;
        Ok(Self {
            cbi_first_id: id,
            cbi_last_id: id,
            cbi_last_id_shared: false,
            cbi_command_queue: vec![SendCommandPointer {
                scp_command: Arc::new(send_command),
                scp_data_payload_start_offset: 0,
                scp_data_payload_end_offset: data_payload_size,
            }],
            cbi_data_payload_size: data_payload_size,
        })
    }
    fn can_append(&self, context: &SendStreamUpgradeContext, other: &Self) -> anyhow::Result<bool> {
        let length = self.cbi_command_queue.len();
        let last_pointer = &self.cbi_command_queue[length - 1];
        let last_command = last_pointer.scp_command.as_ref();
        /*
         * If the last command in the current list is a fragment, then we can
         * only have a single command
         */
        anyhow::ensure!(
            (last_pointer.scp_data_payload_start_offset == 0
                && last_pointer.scp_data_payload_end_offset
                    == last_command.get_cached_data_payload_size()?)
                || length == 1,
            "Found fragment command with others in batch {}",
            self
        );
        let other_length = other.cbi_command_queue.len();
        let other_pointer = &other.cbi_command_queue[other_length - 1];
        let other_command = other_pointer.scp_command.as_ref();
        anyhow::ensure!(
            other_pointer.scp_data_payload_start_offset == 0
                && other_pointer.scp_data_payload_end_offset
                    == other_command.get_cached_data_payload_size()?
                && other_length == 1,
            "Appending bad batch {}",
            other
        );
        // Sanity check the size
        anyhow::ensure!(
            self.cbi_data_payload_size <= context.ssuc_options.maximum_batched_extent_size,
            "Found a command batch {} bigger than batch limit {}",
            self,
            context.ssuc_options.maximum_batched_extent_size
        );
        // If the current batch is already at the maximum size, then disallow
        // the append
        if self.cbi_data_payload_size == context.ssuc_options.maximum_batched_extent_size {
            return Ok(false);
        }
        // Check to see if the commands can be appended and that they are
        // contiguous
        Ok(last_command.can_append(other_command) && (self.cbi_last_id + 1) == other.cbi_first_id)
    }
    /// Try to append the commands together
    /// This will return a tuple with a command to dispatch and a remainder
    pub fn try_append(
        mut self,
        context: &SendStreamUpgradeContext,
        other: Self,
    ) -> anyhow::Result<(Option<Self>, Self)> {
        // If we cannot append the elements, dispatch the previous command
        if !self.can_append(context, &other)? {
            // Dispatch self, keep other
            return Ok((Some(self), other));
        }
        let mut other = other;
        let mut other_pointer = other.cbi_command_queue.remove(0);
        let other_command = other_pointer.scp_command.clone();
        // Check to see if the command needs to be split or not
        if (self.cbi_data_payload_size + other.cbi_data_payload_size)
            <= context.ssuc_options.maximum_batched_extent_size
        {
            // Add the command to the batch
            self.cbi_command_queue.push(other_pointer);
            self.cbi_data_payload_size += other.cbi_data_payload_size;
            self.cbi_last_id += 1;
            // Dispatch none, keep self
            return Ok((None, self));
        }
        // The command needs to be split
        let bytes_to_append =
            context.ssuc_options.maximum_batched_extent_size - self.cbi_data_payload_size;
        self.cbi_command_queue.push(SendCommandPointer {
            scp_command: other_command,
            scp_data_payload_start_offset: 0,
            scp_data_payload_end_offset: bytes_to_append,
        });

        // Attach "bytes_to_append" to the old batch
        self.cbi_data_payload_size += bytes_to_append;
        self.cbi_last_id += 1;
        // The last id will now be shared between this batch and the next one
        self.cbi_last_id_shared = true;

        // Remove "bytes_to_append" from the new batch
        other_pointer.scp_data_payload_start_offset = bytes_to_append;
        other.cbi_command_queue.push(other_pointer);
        other.cbi_data_payload_size -= bytes_to_append;
        anyhow::ensure!(
            other.cbi_last_id == other.cbi_first_id,
            "Found a malformed other command {}",
            other
        );
        // Dispatch self, keep other
        Ok((Some(self), other))
    }
    pub fn remove_first(
        &mut self,
        context: &mut SendStreamUpgradeContext,
    ) -> anyhow::Result<SendCommand> {
        let command_pointer = self.cbi_command_queue.remove(0);
        let command = command_pointer.scp_command;
        // If we own the entire command, just return it
        if command_pointer.scp_data_payload_start_offset == 0
            && command_pointer.scp_data_payload_end_offset
                == command.get_cached_data_payload_size()?
        {
            match Arc::try_unwrap(command) {
                Ok(inner_command) => return Ok(inner_command),
                Err(_) => anyhow::bail!("Unwrapping command with bad strong count"),
            }
        }
        // Note: Maybe in the future we can also check to see if we own the
        // first half of the command and unwrap and return with it
        //
        // Experimenting with that did not really seem to yield a meaningful
        // performance difference
        //
        // Keeping the code simpler for now by always copying here
        command.copy_range(
            context,
            command_pointer.scp_data_payload_start_offset,
            command_pointer.scp_data_payload_end_offset,
        )
    }
    pub fn is_end(&self) -> bool {
        self.cbi_command_queue[0].scp_command.is_end()
    }
    pub fn is_appendable(&self) -> bool {
        self.cbi_command_queue[0].scp_command.is_appendable()
    }
    pub fn is_empty(&self) -> bool {
        self.cbi_command_queue.is_empty()
    }
    pub fn get_cached_data_payload_size(&self) -> usize {
        self.cbi_data_payload_size
    }
    pub fn repopulate(&mut self, command: SendCommand) -> anyhow::Result<()> {
        anyhow::ensure!(
            self.cbi_command_queue.is_empty(),
            "Repopulating a non-empty queue"
        );
        let data_payload_size = command.get_cached_data_payload_size()?;
        self.cbi_command_queue.push(SendCommandPointer {
            scp_command: Arc::new(command),
            scp_data_payload_start_offset: 0,
            scp_data_payload_end_offset: data_payload_size,
        });
        Ok(())
    }
    fn fmt_internal(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<CommandBatchInfo FirstId={} LastId={} PayloadSize={} CommandQueue={:?}/>",
            self.cbi_first_id, self.cbi_last_id, self.cbi_data_payload_size, self.cbi_command_queue,
        )
    }
    pub fn get_command_ref(&self) -> anyhow::Result<&SendCommand> {
        Ok(self.cbi_command_queue[0].scp_command.as_ref())
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

impl Debug for SendCommandPointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl Display for SendCommandPointer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl SendCommandPointer {
    fn fmt_internal(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<Wrapped Command={} Start={} End={}/>",
            self.scp_command, self.scp_data_payload_start_offset, self.scp_data_payload_end_offset,
        )
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
        self.cbi_last_id_shared
    }
}
