/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use slog::debug;

use crate::send_elements::send_command::SendCommand;
use crate::send_elements::send_header::SendHeader;
use crate::send_elements::send_version::SendVersion;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;
use crate::upgrade::send_stream_upgrade_options::SendStreamUpgradeOptions;

pub struct SendStream<'a> {
    /// The global context for processing the stream
    ss_context: SendStreamUpgradeContext<'a>,
}

impl SendStream<'_> {
    pub fn new(options: SendStreamUpgradeOptions) -> anyhow::Result<Self> {
        let context = SendStreamUpgradeContext::new(options)?;
        Ok(SendStream {
            ss_context: context,
        })
    }

    pub fn upgrade(&mut self) -> anyhow::Result<()> {
        // First set the versions on the context
        let header = SendHeader::new(&mut self.ss_context)?;
        let source_version = header.get_version();
        let destination_version = SendVersion::SendVersion2;
        self.ss_context
            .set_versions(source_version, destination_version);

        SendHeader::persist_header(&mut self.ss_context)?;
        let mut previous_command_option: Option<SendCommand> = None;
        loop {
            let mut command = SendCommand::new(&mut self.ss_context)?;
            // First upgrade the command to v2
            if command.is_upgradeable(&self.ss_context)? {
                command = command.upgrade(&mut self.ss_context)?;
            } else {
                command.fake_an_upgrade(&self.ss_context)?;
            }
            match previous_command_option {
                Some(mut previous_command) => {
                    // Try to append the current command to the previous command
                    if previous_command.can_append(&command) {
                        let logical_bytes_appended =
                            previous_command.append(&mut self.ss_context, &command)?;
                        command.truncate_data_payload_at_start(
                            &mut self.ss_context,
                            logical_bytes_appended,
                        )?;
                    }
                    // If the previous command was filled up or if we didn't end up completely
                    // emptying the current command
                    // TODO: Simplify the logic here; if all non-appendable commands are full by
                    // default, then this handling improves
                    // TODO: Remove this check -- it should be sufficient to just check to see if
                    // command is not empty
                    if previous_command.is_full(&self.ss_context) || !command.is_empty() {
                        if previous_command.is_compressible()
                            && self.ss_context.ssuc_options.compression_level != 0
                        {
                            match previous_command.compress(&mut self.ss_context) {
                                Ok(compressed_command) => {
                                    // Successfully compressed command; persist it
                                    compressed_command.persist(&mut self.ss_context)?;
                                }
                                Err(error) => {
                                    match error.downcast_ref::<crate::send_elements::send_attribute::SendAttributeFailedToShrinkPayloadError>() {
                                        Some(failed_to_shrink_payload_error) => {
                                            // If we failed to shrink the attribute payload, just persist the
                                            // old attribute
                                            debug!(self.ss_context.ssuc_logger, "Compress Command Failed: {}; proceeding without compression {}", failed_to_shrink_payload_error, previous_command);
                                            if previous_command.is_dirty() {
                                                // Flush if dirty
                                                previous_command.flush(&mut self.ss_context)?;
                                            }
                                            previous_command.persist(&mut self.ss_context)?;
                                        }
                                        // All other errors should just return failures
                                        None => anyhow::bail!(error),
                                    }
                                }
                            }
                        } else {
                            // Not compressing, but the command might be dirty
                            if previous_command.is_dirty() {
                                // Flush if dirty
                                previous_command.flush(&mut self.ss_context)?;
                            }
                            previous_command.persist(&mut self.ss_context)?;
                        }
                        // Flushed the previous command
                        previous_command_option = None;
                    } else {
                        // Reset the previous command
                        // This is to make rustc happy -- declaring previous_command as mut above
                        // meant that it had to be moved out of previous_command_option
                        // So we now need to move it back
                        previous_command_option = Some(previous_command);
                    }
                }
                None => {}
            }
            // If we're at the end, just persist the current command and break out
            if command.is_end() {
                command.persist(&mut self.ss_context)?;
                break;
            }
            // If the command is not appendable...
            if !command.is_appendable() {
                // Then flush it
                command.persist(&mut self.ss_context)?;
                match previous_command_option {
                    Some(command) => anyhow::bail!("Unexpected previous Command={}", command),
                    None => (),
                }
            } else if !command.is_empty() {
                // Stash a reference to the previous non-empty appendable command
                previous_command_option = Some(command);
            }
        }

        self.ss_context.eprint_summary_stats();

        Ok(())
    }
}
