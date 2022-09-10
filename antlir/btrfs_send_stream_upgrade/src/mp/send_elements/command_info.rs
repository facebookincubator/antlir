/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Display;

use crate::mp::sync::unordered_element::UnorderedElement;
use crate::send_elements::send_command_header::SendCommandHeader;

pub struct CommandInfo {
    /// The id of the command
    ci_id: u64,
    /// The header that is associated with the command to be built
    ci_send_command_header: SendCommandHeader,
    /// The start address of the payload buffer
    ci_buffer_start_address: usize,
}

impl CommandInfo {
    pub fn new(
        id: u64,
        send_command_header: SendCommandHeader,
        start_address: usize,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            ci_send_command_header: send_command_header,
            ci_id: id,
            ci_buffer_start_address: start_address,
        })
    }
    pub fn split(self) -> (u64, SendCommandHeader, usize) {
        (
            self.ci_id,
            self.ci_send_command_header,
            self.ci_buffer_start_address,
        )
    }
}

unsafe impl Send for CommandInfo {}

impl UnorderedElement for CommandInfo {}

impl Display for CommandInfo {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<CommandInfo Id={} Header={} Start={}/>",
            self.ci_id, self.ci_send_command_header, self.ci_buffer_start_address,
        )
    }
}
