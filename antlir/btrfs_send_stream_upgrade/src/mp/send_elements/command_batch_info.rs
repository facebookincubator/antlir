/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Display;

use crate::mp::sync::ordered_element::OrderedElement;
use crate::mp::sync::unordered_element::UnorderedElement;
use crate::send_elements::send_command::SendCommand;

pub struct CommandBatchInfo {
    /// The first id associated with the batch
    cbi_first_id: u64,
    /// The last id associated with the batch
    cbi_last_id: u64,
    /// A list of commands to be batched together
    cbi_command_queue: Vec<SendCommand>,
}

impl CommandBatchInfo {
    pub fn new(id: u64, send_command: SendCommand) -> anyhow::Result<Self> {
        Ok(Self {
            cbi_first_id: id,
            cbi_last_id: id,
            cbi_command_queue: vec![send_command],
        })
    }
    // To add: More methods like can append, append, flatten
}

impl UnorderedElement for CommandBatchInfo {}

unsafe impl Send for CommandBatchInfo {}

impl Display for CommandBatchInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<CommandBatchInfo FirstId={} LastId={} CommandQueue={:?}/>",
            self.cbi_first_id, self.cbi_last_id, self.cbi_command_queue,
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
        // Verify this while in append and can append
        false
    }
}
