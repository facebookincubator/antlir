/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::fmt::Display;
use std::time::SystemTime;

use anyhow::Context;
use slog::debug;
use slog::info;
use slog::trace;

use crate::send_elements::send_attribute::SendAttribute;
use crate::send_elements::send_attribute_header::BtrfsSendAttributeType;
use crate::send_elements::send_attribute_header::BTRFS_ENCODED_IO_COMPRESSION_ZSTD;
use crate::send_elements::send_command_header::SendCommandHeader;
use crate::send_elements::send_version::SendVersion;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

const BLOCK_SIZE: usize = 4096;

pub struct SendCommand {
    /// The header for the current command
    sc_header: SendCommandHeader,
    /// A vector containing a buffer for the entire command
    /// This includes the header
    /// Note that if we have a data attribute, this will not hold the data attribute
    /// until the command is actually persisted
    sc_buffer: Vec<u8>,
    /// The data attribute for the current command
    sc_data_attribute: Option<SendAttribute>,
    /// The initial size for the data attribute
    sc_data_attribute_initial_size: Option<usize>,
    /// Whether the data attribute is dirty or not
    sc_data_attribute_dirty: bool,
    /// The path for the current command
    sc_path: Option<String>,
    /// The start offset for the current command
    sc_start_offset: Option<usize>,
    /// The uncompressed size of the entire command
    sc_uncompressed_size: usize,
    /// The version of the command
    sc_version: SendVersion,
}

impl Display for SendCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl Debug for SendCommand {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl SendCommand {
    pub fn new(context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        // Read in the header
        let header = SendCommandHeader::new(context)?;
        // Generate the command from the header
        Self::new_from_header(context, header)
    }

    pub fn new_from_header(
        context: &mut SendStreamUpgradeContext,
        header: SendCommandHeader,
    ) -> anyhow::Result<Self> {
        context.trace_stats();
        let version = context.get_source_version()?;
        let header_size = SendCommandHeader::get_header_size();
        // Compute the total size of the command
        let payload_size = header.get_command_payload_size()?;
        let total_size = header_size + payload_size;
        // Set up the command buffer
        let mut buffer = vec![0; total_size];
        let mut data_attribute: Option<SendAttribute> = None;
        let mut data_attribute_initial_size: Option<usize> = None;
        let mut path: Option<String> = None;
        let mut start_offset: Option<usize> = None;
        // Copy over the entire command in the given buffer
        context.read_exact(&mut buffer[header_size..])?;

        {
            // Iterate through the current buffer to generate attributes
            let mut sub_context = context.clone_with_new_buffers(
                Some(&buffer[header_size..]),
                None,
                version,
                version,
            );
            let mut buffer_offset = 0;
            // Walk through the entire payload
            while buffer_offset < payload_size {
                anyhow::ensure!(data_attribute.is_none(), "Data attribute must be set last");
                // Build an attribute
                let attribute = SendAttribute::new(&mut sub_context)?;
                let attribute_size = attribute.get_size();
                if attribute.is_send_a_path() {
                    // Cache the path
                    path = Some(attribute.get_payload_as_string()?);
                } else if attribute.is_send_a_file_offset() {
                    // Cache the offset
                    start_offset = Some(attribute.get_payload_as_u64()? as usize);
                } else if attribute.is_send_a_data() {
                    // Cache the entire data attribute
                    data_attribute = Some(attribute);
                    data_attribute_initial_size = Some(attribute_size);
                }
                buffer_offset += attribute_size;
            }
            context.return_child(&mut sub_context);
        }

        // Normally, we want to check crcs when we read new commands
        if !context.ssuc_options.avoid_crcing_input {
            // Create a new context for persisting the header
            {
                // Persist the header
                let mut sub_context =
                    context.clone_with_new_buffers(None, Some(&mut buffer), version, version);
                // Skip the CRC32C -- we don't want to compute it yet
                header.persist(&mut sub_context, true)?;
                context.return_child(&mut sub_context);
            }

            // Let's compute the CRC32C to ensure that it matches
            let stored_crc32c = header.get_crc32c()?;
            let computed_crc32c = Self::compute_crc32c(context, &buffer);
            anyhow::ensure!(
                stored_crc32c == computed_crc32c,
                "Mismatch between stored CRC32C {:#010X} and computed CRC32C {:#010X}",
                stored_crc32c,
                computed_crc32c
            );
        }

        // Rewrite the entire command to the buffer to write in the crc this time
        {
            // Persist the header
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(&mut buffer), version, version);
            // Flush the CRC32C this time
            header.persist(&mut sub_context, false)?;
            context.return_child(&mut sub_context);
        }

        let command = SendCommand {
            sc_header: header,
            sc_buffer: buffer,
            sc_data_attribute: data_attribute,
            sc_data_attribute_initial_size: data_attribute_initial_size,
            sc_data_attribute_dirty: false,
            sc_path: path,
            sc_start_offset: start_offset,
            sc_uncompressed_size: total_size,
            sc_version: version,
        };
        command.verify(context)?;
        info!(context.ssuc_logger, "New Command={}", command);
        Ok(command)
    }

    /// Copies a range of a given command into a new command
    pub fn copy_range(
        &self,
        context: &mut SendStreamUpgradeContext,
        data_payload_start_offset: usize,
        data_payload_end_offset: usize,
    ) -> anyhow::Result<Self> {
        context.trace_stats();
        info!(
            context.ssuc_logger,
            "Copying range Command={} Start={} End={}",
            self,
            data_payload_start_offset,
            data_payload_end_offset
        );
        // Sanity check
        anyhow::ensure!(
            !self.sc_data_attribute_dirty,
            "Copying range on dirty command {} not yet supported",
            self,
        );
        anyhow::ensure!(
            self.is_appendable(),
            "Copying range on non-appendable command {}",
            self,
        );
        let version = self.sc_version;
        let old_data_attribute = match &self.sc_data_attribute {
            Some(attribute) => attribute,
            None => anyhow::bail!(
                "Trying to copy range from Command={} with None data attribute",
                self
            ),
        };
        match self.sc_data_attribute_initial_size {
            Some(size) => anyhow::ensure!(
                size == old_data_attribute.get_size(),
                "Found clean command {} with bad attribute size",
                self,
            ),
            None => anyhow::bail!(
                "Trying to copy range Command={} with None data attribute initial size",
                self
            ),
        }
        let header_size = SendCommandHeader::get_header_size();
        let old_payload_size = self.sc_header.get_command_payload_size()?;
        let old_total_size = header_size + old_payload_size;
        let old_data_payload_size = old_data_attribute.get_payload_size();
        let new_data_payload_size = data_payload_end_offset - data_payload_start_offset;
        // The size difference should be equal to the difference in payload
        // sizes -- only the size of the data attribute is changing
        let new_total_size = old_total_size + new_data_payload_size - old_data_payload_size;
        let new_payload_size = new_total_size - header_size;

        // Set up the new header
        let header = self
            .sc_header
            .copy(context, Some(new_payload_size as u32))?;

        // Set up the command buffer
        let mut buffer = vec![0; new_total_size];

        // Persist the header to the buffer
        {
            let mut sub_context = context.clone_with_new_buffers(
                None,
                Some(&mut buffer[0..header_size]),
                version,
                version,
            );
            header.persist(&mut sub_context, true)?;
            context.return_child(&mut sub_context);
        }

        // Flush the pre-data region too
        // It's okay that the file offset is wrong, since we'll be marking the
        // command as dirty
        // Allow the flush to proceed despite the fact that the underlying
        // command is clean
        self.flush_pre_data_to_buffer(context, &mut buffer, true)?;

        // Populate all of the new attributes
        // The path is the same
        let path = match self.sc_path {
            Some(ref path) => Some(path.clone()),
            None => anyhow::bail!("Copying range from command {} with None path", self),
        };
        // Adjust the offset by the payload offset
        let start_offset = match self.sc_start_offset {
            Some(offset) => Some(offset + data_payload_start_offset),
            None => anyhow::bail!("Copying range from command {} with None offset", self),
        };
        // Copy over the right amount of the data attribute
        let data_attribute = old_data_attribute.copy_range(
            context,
            data_payload_start_offset,
            data_payload_end_offset,
        )?;
        let data_attribute_initial_size = data_attribute.get_size();

        // Set the new command to dirty -- we haven't copied over the data
        // payload to the command buffer
        let command = SendCommand {
            sc_header: header,
            sc_buffer: buffer,
            sc_data_attribute: Some(data_attribute),
            sc_data_attribute_initial_size: Some(data_attribute_initial_size),
            sc_data_attribute_dirty: true,
            sc_path: path,
            sc_start_offset: start_offset,
            sc_uncompressed_size: new_total_size,
            sc_version: version,
        };
        command.verify(context)?;
        info!(context.ssuc_logger, "Copied range to Command={}", command);
        Ok(command)
    }

    pub fn upgrade(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        let source_version = self.sc_version;
        let destination_version = context.get_destination_version()?;
        info!(
            context.ssuc_logger,
            "Upgrading Command={} to version={}", self, destination_version
        );
        anyhow::ensure!(
            self.is_upgradeable(context)?,
            "Trying to upgrade an unupgradeable Command={}",
            self
        );
        // Construct the new command header
        let mut upgraded_header = self.sc_header.upgrade(context)?;
        // Let's create the compression buffer
        // Initially assume that we won't be able to get any space savings
        let header_size = SendCommandHeader::get_header_size();
        let old_payload_size = self.sc_header.get_command_payload_size()?;
        let old_total_size = header_size + old_payload_size;
        let mut upgraded_buffer = vec![0; old_total_size];
        let mut data_attribute: Option<SendAttribute> = None;
        let mut data_attribute_initial_size: Option<usize> = None;
        // Iterate through the byte array of the source
        // Skip past the headers
        let mut old_offset = header_size;
        let mut new_offset = header_size;

        {
            let mut sub_context = context.clone_with_new_buffers(
                Some(&self.sc_buffer[header_size..]),
                Some(&mut upgraded_buffer[header_size..]),
                source_version,
                destination_version,
            );
            while old_offset < old_total_size {
                sub_context.trace_stats();
                debug!(
                    sub_context.ssuc_logger,
                    "Processing attributes at offsets new {}B old {}B / {}B",
                    new_offset,
                    old_offset,
                    old_total_size
                );
                let mut attribute = SendAttribute::new(&mut sub_context)?;
                if attribute.is_upgradeable(context)? {
                    // Found an upgradeable attribute
                    attribute = attribute.upgrade(&mut sub_context)?;
                } else {
                    attribute.fake_an_upgrade(&sub_context)?;
                }
                attribute.persist(&mut sub_context)?;
                if attribute.is_send_a_data() {
                    let size = attribute.get_size();
                    data_attribute = Some(attribute);
                    data_attribute_initial_size = Some(size);
                } else {
                    anyhow::ensure!(
                        !attribute.is_compressible(),
                        "No handling for compessible non-data Attribute={}",
                        attribute
                    );
                }
                old_offset = sub_context.get_read_offset() + header_size;
                new_offset = sub_context.get_write_offset() + header_size;
            }
            context.return_child(&mut sub_context);
        }

        anyhow::ensure!(
            old_offset == old_total_size,
            "Mismatch between offset {}B and command size {}B",
            old_offset,
            old_total_size
        );
        // We've written everything out except for the header
        // Update the size of the command payload so that we can persist it
        let new_payload_size = new_offset as u32 - header_size as u32;
        debug!(
            context.ssuc_logger,
            "Resizing command payload from {}B to {}B", old_payload_size, new_payload_size
        );
        upgraded_header.set_size(new_payload_size)?;
        // Also update the size of the buffer
        debug!(
            context.ssuc_logger,
            "Shrinking command buffer from {}B to {}B",
            upgraded_buffer.len(),
            new_offset
        );
        upgraded_buffer.truncate(new_offset);

        {
            // Persist the upgraded header
            let mut sub_context = context.clone_with_new_buffers(
                None,
                Some(&mut upgraded_buffer),
                source_version,
                destination_version,
            );
            // Skip the CRC32C for now (since we don't know what it should be)
            upgraded_header.persist(&mut sub_context, true)?;
            context.return_child(&mut sub_context);
        }

        // Compute the CRC32C
        let computed_crc32c = Self::compute_crc32c(context, &upgraded_buffer);
        // Update the CRC32C and persist the header
        upgraded_header.set_crc32c(computed_crc32c)?;

        {
            // Persist the upgraded header
            let mut sub_context = context.clone_with_new_buffers(
                None,
                Some(&mut upgraded_buffer),
                source_version,
                destination_version,
            );
            // Flush the CRC32C this time
            upgraded_header.persist(&mut sub_context, false)?;
            context.return_child(&mut sub_context);
        }

        let path: Option<String> = self.sc_path.as_ref().cloned();
        let new_command = SendCommand {
            sc_header: upgraded_header,
            sc_buffer: upgraded_buffer,
            sc_data_attribute: data_attribute,
            sc_data_attribute_initial_size: data_attribute_initial_size,
            sc_data_attribute_dirty: false,
            sc_uncompressed_size: new_offset,
            sc_version: destination_version,
            sc_path: path,
            ..*self
        };
        new_command.verify(context)?;
        info!(context.ssuc_logger, "Upgraded  NewCommand={}", new_command);
        Ok(new_command)
    }

    pub fn fake_an_upgrade(&mut self, context: &SendStreamUpgradeContext) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.is_upgradeable(context)?,
            "Trying to fake upgrade an upgradeable Command={}",
            self
        );
        // Upgrade the header too
        self.sc_header.fake_an_upgrade(context)?;
        self.sc_version = context.get_destination_version()?;
        Ok(())
    }

    fn flush_pre_data_to_buffer(
        &self,
        context: &mut SendStreamUpgradeContext,
        buffer: &mut [u8],
        allow_clean: bool,
    ) -> anyhow::Result<()> {
        context.trace_stats();
        info!(
            context.ssuc_logger,
            "Flushing pre-data for Command={}", self
        );
        let data_attribute_initial_size = match self.sc_data_attribute_initial_size {
            Some(size) => size,
            None => anyhow::bail!(
                "Trying to compress Command={} without a data attribute initial size",
                self
            ),
        };
        let source_version = self.sc_version;
        let destination_version = context.get_destination_version()?;
        let header_size = SendCommandHeader::get_header_size();
        let old_payload_size = self.sc_header.get_command_payload_size()?;
        let old_total_size = header_size + old_payload_size;
        let old_pre_data_size = old_total_size - data_attribute_initial_size;
        let command_start_offset = match self.sc_start_offset {
            Some(offset) => offset,
            None => anyhow::bail!("Trying to flush Command={} without a start offset", self),
        };
        anyhow::ensure!(
            self.sc_data_attribute_dirty || allow_clean,
            "Unnecessary pre-data flush on non-dirty Command={}",
            self
        );
        let mut sub_context = context.clone_with_new_buffers(
            Some(&self.sc_buffer[header_size..old_pre_data_size]),
            Some(&mut buffer[header_size..old_pre_data_size]),
            source_version,
            destination_version,
        );

        // Iterate through the attributes
        // Create attributes and flush them accordingly
        // Note that we need to create a new command start offset attribute; the data attribute was
        // dirty, so this value could have changed
        let mut read_offset = header_size;
        while read_offset < old_pre_data_size {
            sub_context.trace_stats();
            debug!(
                sub_context.ssuc_logger,
                "Flushing pre-data attributes at offset {}B / {}B", read_offset, old_pre_data_size
            );
            // Build the attribute
            let mut attribute = SendAttribute::new(&mut sub_context)?;
            // Verify it
            attribute.verify(&mut sub_context)?;
            // Copy everything over except for the file offset -- this could
            // have changed; grab this from the command instead
            if attribute.is_send_a_file_offset() {
                // Flush the new command offset
                let command_start_offset_attribute = SendAttribute::new_from_u64(
                    &mut sub_context,
                    BtrfsSendAttributeType::BTRFS_SEND_A_FILE_OFFSET,
                    command_start_offset as u64,
                )?;
                command_start_offset_attribute.persist(&mut sub_context)?;
            } else {
                // Otherwise, persist the attribute that we found
                attribute.persist(&mut sub_context)?;
            }
            // Note that we always need to skip the header
            read_offset = sub_context.get_read_offset() + header_size;
            let write_offset = sub_context.get_write_offset() + header_size;
            // Third check -- the two offsets must match
            anyhow::ensure!(
                read_offset == write_offset,
                "Flushing pre-data attributes; Command={} {}B read offset, {}B write offset",
                self,
                read_offset,
                write_offset
            );
        }

        context.return_child(&mut sub_context);
        info!(context.ssuc_logger, "Flushed pre-data for Command={}", self);
        Ok(())
    }

    fn flush_header_to_buffer(
        context: &mut SendStreamUpgradeContext,
        buffer: &mut [u8],
        header: &mut SendCommandHeader,
    ) -> anyhow::Result<()> {
        context.trace_stats();
        let version = context.get_destination_version()?;
        info!(context.ssuc_logger, "Flushing Header={}", header);

        {
            // Persist the header
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(buffer), version, version);
            // Skip the CRC32C for now (since we don't know what it should be)
            header.persist(&mut sub_context, true)?;
            context.return_child(&mut sub_context);
        }

        // Compute the CRC32C
        let computed_crc32c = Self::compute_crc32c(context, buffer);
        // Update the CRC32C and persist the header
        header.set_crc32c(computed_crc32c)?;

        {
            // Persist the header
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(buffer), version, version);
            // Flush the CRC32C this time
            header.persist(&mut sub_context, false)?;
            context.return_child(&mut sub_context);
        }

        info!(context.ssuc_logger, "Flushed Header={}", header);
        Ok(())
    }

    pub fn flush(&mut self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<()> {
        context.trace_stats();
        let source_version = self.sc_version;
        let destination_version = context.get_destination_version()?;
        info!(context.ssuc_logger, "Flushing Command={}", self);
        anyhow::ensure!(
            self.sc_data_attribute_dirty,
            "Unnecessary flush on non-dirty Command={}",
            self
        );
        let data_attribute = match &self.sc_data_attribute {
            Some(attribute) => attribute,
            None => anyhow::bail!(
                "Trying to compress Command={} without a data attribute",
                self
            ),
        };
        let data_attribute_initial_size = match self.sc_data_attribute_initial_size {
            Some(size) => size,
            None => anyhow::bail!(
                "Trying to compress Command={} without a data attribute initial size",
                self
            ),
        };
        let mut flush_header = self.sc_header.copy(context, None)?;
        let header_size = SendCommandHeader::get_header_size();
        let old_payload_size = self.sc_header.get_command_payload_size()?;
        let old_total_size = header_size + old_payload_size;
        let old_pre_data_size = old_total_size - data_attribute_initial_size;
        let data_attribute_size = data_attribute.get_size();
        let new_total_size = old_pre_data_size + data_attribute_size;
        let new_payload_size = new_total_size as u32 - header_size as u32;
        let mut buffer = vec![0; new_total_size];

        // First, flush everything before the data attribute
        self.flush_pre_data_to_buffer(context, &mut buffer, false)?;

        // Next, flush the data attribute
        {
            let mut sub_context = context.clone_with_new_buffers(
                Some(&self.sc_buffer[old_pre_data_size..]),
                Some(&mut buffer[old_pre_data_size..]),
                source_version,
                destination_version,
            );
            // Persist the compressed attribute
            data_attribute.persist(&mut sub_context)?;
            context.return_child(&mut sub_context);
        }

        // Finally, flush the header
        flush_header.set_size(new_payload_size)?;
        Self::flush_header_to_buffer(context, &mut buffer, &mut flush_header)?;
        // The command is now clean; update fields accordingly
        self.sc_header = flush_header;
        self.sc_buffer = buffer;
        self.sc_data_attribute_initial_size = Some(data_attribute_size);
        self.sc_data_attribute_dirty = false;
        Ok(())
    }

    pub fn compress(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        let source_version = self.sc_version;
        let destination_version = context.get_destination_version()?;
        info!(context.ssuc_logger, "Compressing Command={}", self);
        anyhow::ensure!(
            self.is_compressible(),
            "Trying to compress an uncompressible Command={}",
            self
        );
        let data_attribute = match &self.sc_data_attribute {
            Some(attribute) => attribute,
            None => anyhow::bail!(
                "Trying to compress Command={} without a data attribute",
                self
            ),
        };
        let data_attribute_initial_size = match self.sc_data_attribute_initial_size {
            Some(size) => size,
            None => anyhow::bail!(
                "Trying to compress Command={} without a data attribute initial size",
                self
            ),
        };
        anyhow::ensure!(
            data_attribute.is_compressible(),
            "Trying to compress an uncompressible Attribute={}",
            data_attribute
        );
        // Construct the new command header
        let mut compressed_header = self.sc_header.compress(context)?;
        // Let's create the compression buffer
        // Initially assume that we won't be able to get any space savings
        let header_size = SendCommandHeader::get_header_size();
        let old_payload_size = self.sc_header.get_command_payload_size()?;
        let old_total_size = header_size + old_payload_size;
        let old_pre_data_size = old_total_size - data_attribute_initial_size;
        let new_max_total_size = old_pre_data_size + data_attribute.get_size();
        let new_max_payload_size = new_max_total_size as u32 - header_size as u32;
        let mut compressed_buffer = vec![0; new_max_total_size];

        if !self.sc_data_attribute_dirty {
            let mut sub_context = context.clone_with_new_buffers(
                Some(&self.sc_buffer[header_size..old_pre_data_size]),
                None,
                source_version,
                destination_version,
            );
            // If the data attribute wasn't dirtied, then copy everything over
            sub_context.read_exact(&mut compressed_buffer[header_size..old_pre_data_size])?;
            context.return_child(&mut sub_context);
        } else {
            // Otherwise, we need to flush everything except for the data attribute
            // That will potentially be compressed first
            self.flush_pre_data_to_buffer(context, &mut compressed_buffer, false)?;
        }

        // Attempt to compress the attribute and persist it to the compressed_buffer
        let mut extra_bytes_written;
        let compressed_data_attribute: SendAttribute;
        let compressed_data_attribute_initial_size: usize;

        {
            let mut sub_context = context.clone_with_new_buffers(
                Some(&self.sc_buffer[old_pre_data_size..]),
                Some(&mut compressed_buffer[old_pre_data_size..]),
                source_version,
                destination_version,
            );
            // First persist additional attributes
            // Track the number of bytes that were written -- we want to ensure that our command
            // is smaller than before
            let start_offset = sub_context.get_write_offset();
            let uncompressed_size = data_attribute.get_uncompressed_payload_size() as u64;
            let metadata_attribute = SendAttribute::new_from_u64(
                &mut sub_context,
                BtrfsSendAttributeType::BTRFS_SEND_A_UNENCODED_FILE_LEN,
                uncompressed_size,
            )?;
            metadata_attribute.persist(&mut sub_context)?;
            let metadata_attribute = SendAttribute::new_from_u64(
                &mut sub_context,
                BtrfsSendAttributeType::BTRFS_SEND_A_UNENCODED_LEN,
                uncompressed_size,
            )?;
            metadata_attribute.persist(&mut sub_context)?;
            let metadata_attribute = SendAttribute::new_from_u64(
                &mut sub_context,
                BtrfsSendAttributeType::BTRFS_SEND_A_UNENCODED_OFFSET,
                0,
            )?;
            metadata_attribute.persist(&mut sub_context)?;
            let metadata_attribute = SendAttribute::new_from_u32(
                &mut sub_context,
                BtrfsSendAttributeType::BTRFS_SEND_A_COMPRESSION,
                BTRFS_ENCODED_IO_COMPRESSION_ZSTD,
            )?;
            metadata_attribute.persist(&mut sub_context)?;
            extra_bytes_written = sub_context.get_write_offset() - start_offset;

            match data_attribute.compress(&mut sub_context, extra_bytes_written) {
                Ok(compressed_attribute) => {
                    // Persist the compressed attribute too
                    compressed_attribute.persist(&mut sub_context)?;
                    compressed_data_attribute = compressed_attribute;
                    compressed_data_attribute_initial_size = compressed_data_attribute.get_size();
                }
                Err(error) => {
                    // This could be a recoverable error, so return the sub context
                    context.return_child(&mut sub_context);
                    anyhow::bail!(error);
                }
            }
            extra_bytes_written = sub_context.get_write_offset() - start_offset;
            context.return_child(&mut sub_context);
        }

        // We've written everything out except for the header
        // Update the size of the command payload so that we can persist it
        let new_total_size = old_pre_data_size + extra_bytes_written;
        let new_payload_size = new_total_size as u32 - header_size as u32;
        debug!(
            context.ssuc_logger,
            "Resizing command payload from {}B to {}B", old_payload_size, new_payload_size
        );
        anyhow::ensure!(
            new_max_payload_size >= new_payload_size,
            "Command size increased from {}B to {}B",
            new_max_payload_size,
            new_payload_size
        );
        compressed_header.set_size(new_payload_size)?;
        // Also update the size of the buffer
        debug!(
            context.ssuc_logger,
            "Shrinking command buffer from {}B to {}B",
            compressed_buffer.len(),
            new_total_size
        );
        anyhow::ensure!(
            compressed_buffer.len() >= new_total_size,
            "Buffer size increased from {}B to {}B",
            compressed_buffer.len(),
            new_total_size
        );
        compressed_buffer.truncate(new_total_size);
        Self::flush_header_to_buffer(context, &mut compressed_buffer, &mut compressed_header)?;
        let path: Option<String> = self.sc_path.as_ref().cloned();
        let new_command = SendCommand {
            sc_header: compressed_header,
            sc_buffer: compressed_buffer,
            sc_data_attribute: Some(compressed_data_attribute),
            sc_data_attribute_initial_size: Some(compressed_data_attribute_initial_size),
            sc_data_attribute_dirty: false,
            sc_version: destination_version,
            sc_path: path,
            ..*self
        };
        new_command.verify(context)?;
        info!(context.ssuc_logger, "Compressed NewCommand={}", new_command);
        Ok(new_command)
    }

    pub fn verify(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<()> {
        if !context.ssuc_options.serde_checks {
            return Ok(());
        }
        context.trace_stats();
        let version = self.sc_version;
        info!(
            context.ssuc_logger,
            "Checking Command={} destination verion={}", self, version
        );
        let header_size = SendCommandHeader::get_header_size();
        let source_buffer;
        let mut flushed_buffer;

        if !self.sc_data_attribute_dirty {
            // If the data attribute is not dirty, then just have the source buffer point to
            // the command source buffer
            source_buffer = &self.sc_buffer;
        } else {
            // If the data buffer is dirty, then we must "flush" it
            let dirty_data_attribute = match &self.sc_data_attribute {
                Some(attribute) => attribute,
                None => anyhow::bail!("Verifying dirty Command={} without a data attribute", self),
            };
            let dirty_data_attribute_initial_size = match self.sc_data_attribute_initial_size {
                Some(size) => size,
                None => anyhow::bail!(
                    "Verifying dirty Command={} without a data attribute initial size",
                    self
                ),
            };
            let command_start_offset = match self.sc_start_offset {
                Some(offset) => offset,
                None => anyhow::bail!("Trying to compress Command={} without a start offset", self),
            };
            let new_pre_data_attribute_size =
                self.sc_buffer.len() - dirty_data_attribute_initial_size;
            let new_total_size = new_pre_data_attribute_size + dirty_data_attribute.get_size();
            // Create the flushed buffer
            flushed_buffer = vec![0; new_total_size];
            flushed_buffer[..header_size].copy_from_slice(&self.sc_buffer[..header_size]);

            {
                let mut sub_context = context.clone_with_new_buffers(
                    Some(&self.sc_buffer[header_size..new_pre_data_attribute_size]),
                    Some(&mut flushed_buffer[header_size..new_pre_data_attribute_size]),
                    version,
                    version,
                );
                let mut read_offset = header_size;
                while read_offset < new_pre_data_attribute_size {
                    sub_context.trace_stats();
                    debug!(
                        sub_context.ssuc_logger,
                        "Checking attributes for flush at offset {}B / {}B",
                        read_offset,
                        new_pre_data_attribute_size
                    );
                    // Build the attribute
                    let mut attribute = SendAttribute::new(&mut sub_context)?;
                    // Verify it
                    attribute.verify(&mut sub_context)?;
                    // Flush the new command offset
                    if attribute.is_send_a_file_offset() {
                        let command_start_offset_attribute = SendAttribute::new_from_u64(
                            &mut sub_context,
                            BtrfsSendAttributeType::BTRFS_SEND_A_FILE_OFFSET,
                            command_start_offset as u64,
                        )?;
                        command_start_offset_attribute.persist(&mut sub_context)?;
                    } else {
                        attribute.persist(&mut sub_context)?;
                    }
                    // Note that we always need to skip the header
                    read_offset = sub_context.get_read_offset() + header_size;
                    let write_offset = sub_context.get_write_offset() + header_size;
                    // Third check -- the two offsets must match
                    anyhow::ensure!(
                        read_offset == write_offset,
                        "Checking command failed during flush; {}B read offset, {}B write offset",
                        read_offset,
                        write_offset
                    );
                }
                context.return_child(&mut sub_context);
            }

            // Create a sub context based to persist the attribute to the flushed buffer
            {
                let mut sub_context = context.clone_with_new_buffers(
                    None,
                    Some(&mut flushed_buffer[new_pre_data_attribute_size..]),
                    version,
                    version,
                );
                dirty_data_attribute.persist(&mut sub_context)?;
                context.return_child(&mut sub_context);
            }
            source_buffer = &flushed_buffer;
        }
        let header;

        {
            // Create a sub context based on the buffer of the current command
            let mut sub_context =
                context.clone_with_new_buffers(Some(source_buffer), None, version, version);
            // Read in the header
            header = SendCommandHeader::new(&mut sub_context)?;
            context.return_child(&mut sub_context);
        }

        // Compute the total size of the command
        let mut total_size = header_size + header.get_command_payload_size()?;
        // Compute the old total size based on the incoming command
        let old_payload_size = self.sc_header.get_command_payload_size()?;
        // First check -- ensure that the buffer size is correct
        anyhow::ensure!(
            total_size == self.sc_buffer.len(),
            "Verifying command failed; {}B size doesn't match {}B buffer size",
            total_size,
            self.sc_buffer.len()
        );
        // Second check -- the two command sizes must match
        anyhow::ensure!(
            total_size == header_size + old_payload_size,
            "Verifying command failed; {}B after, {}B + {}B before",
            total_size,
            header_size,
            old_payload_size
        );
        // If the attribute is dirty, then adjust the size accordingly
        if self.sc_data_attribute_dirty {
            total_size = source_buffer.len();
        }
        // Now set up a real buffer to hold the new command
        let mut buffer = vec![0; total_size];
        // Iterate through the byte array of the source one attribute at a time
        // Skip past the headers
        let mut read_offset = header_size;
        // Set up the attributes we want to check too
        let mut data_attribute: Option<SendAttribute> = None;
        let mut data_attribute_initial_size: Option<usize> = None;
        let mut path: Option<String> = None;
        let mut command_start_offset: Option<usize> = None;

        {
            let mut sub_context = context.clone_with_new_buffers(
                Some(&source_buffer[header_size..]),
                Some(&mut buffer[header_size..]),
                version,
                version,
            );
            while read_offset < total_size {
                sub_context.trace_stats();
                debug!(
                    sub_context.ssuc_logger,
                    "Checking attributes at offset {}B / {}B", read_offset, total_size
                );
                // Build the attribute
                let mut attribute = SendAttribute::new(&mut sub_context)?;
                // Verify it
                attribute.verify(&mut sub_context)?;
                // Persist the attribute we found
                attribute.persist(&mut sub_context)?;
                // Cache the attribute as necessary
                if attribute.is_send_a_data() {
                    // Cache the entire data attribute
                    let size = attribute.get_size();
                    data_attribute = Some(attribute);
                    data_attribute_initial_size = Some(size);
                } else if attribute.is_send_a_path() {
                    // Cache the path
                    path = Some(attribute.get_payload_as_string()?);
                } else if attribute.is_send_a_file_offset() {
                    // Cache the offset
                    command_start_offset = Some(attribute.get_payload_as_u64()? as usize);
                }
                // Note that we always need to skip the header
                read_offset = sub_context.get_read_offset() + header_size;
                let write_offset = sub_context.get_write_offset() + header_size;
                // Third check -- the two offsets must match
                anyhow::ensure!(
                    read_offset == write_offset,
                    "Verifying command failed; {}B read offset, {}B write offset",
                    read_offset,
                    write_offset
                );
            }
            context.return_child(&mut sub_context);
        }

        // Fourth check -- no overflow
        anyhow::ensure!(
            read_offset == total_size,
            "Verifying command failed; Mismatch between offset {}B and command size {}B",
            read_offset,
            total_size
        );

        {
            // Now persist the header
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(&mut buffer), version, version);
            // Skip the CRC32C if the data attribute was clean (since we don't know what it should be)
            // If the data attribute is dirty, we want to write the crc (since we won't be
            // computing it later)
            header.persist(&mut sub_context, !self.sc_data_attribute_dirty)?;
            context.return_child(&mut sub_context);
        }

        // Before the CRC32 calculation, dump blocks from both the source and destination
        for i in 0..context.ssuc_options.bytes_to_log {
            if i >= buffer.len() || i >= self.sc_buffer.len() {
                break;
            }
            trace!(
                context.ssuc_logger,
                "Buffer bytes {} source {:#04X} destination {:#04X}",
                i,
                self.sc_buffer[i],
                buffer[i]
            );
        }

        // If the data attribute is not dirty, then do a crc check
        if !self.sc_data_attribute_dirty {
            // Let's compute the CRC32C to ensure that it matches
            let stored_crc32c = header.get_crc32c()?;
            let computed_crc32c = Self::compute_crc32c(context, &buffer);
            // Fifth check -- the crc must match
            anyhow::ensure!(
                stored_crc32c == computed_crc32c,
                "Verifying command failed; Mismatch between stored CRC32C {:#010X} and computed CRC32C {:#010X}",
                stored_crc32c,
                computed_crc32c
            );
            {
                // Persist the header
                let mut sub_context =
                    context.clone_with_new_buffers(None, Some(&mut buffer), version, version);
                // Flush the CRC32C this time
                header.persist(&mut sub_context, false)?;
                context.return_child(&mut sub_context);
            }
        }

        let new_command = SendCommand {
            sc_header: header,
            sc_buffer: buffer,
            sc_data_attribute: data_attribute,
            sc_data_attribute_initial_size: data_attribute_initial_size,
            sc_data_attribute_dirty: false,
            sc_path: path,
            sc_start_offset: command_start_offset,
            sc_uncompressed_size: total_size,
            sc_version: version,
        };
        // Sixth check -- the commands must match
        anyhow::ensure!(
            *self == new_command,
            "Verifying command failed; Command={} != ReconstructedCommand={}",
            *self,
            new_command
        );
        info!(context.ssuc_logger, "Passed Check on Command={}", self);
        Ok(())
    }

    pub fn append(
        &mut self,
        context: &mut SendStreamUpgradeContext,
        other: &Self,
    ) -> anyhow::Result<usize> {
        context.trace_stats();
        info!(
            context.ssuc_logger,
            "Appending Command={} with Command={}", self, other
        );
        anyhow::ensure!(
            self.can_append(other),
            "Cannot append Command={} with Command={}",
            self,
            other
        );
        let data_attribute = match &mut self.sc_data_attribute {
            Some(attribute) => attribute,
            None => anyhow::bail!(
                "Trying to append to Command={} without a data attribute",
                self
            ),
        };
        let other_data_attribute = match &other.sc_data_attribute {
            Some(attribute) => attribute,
            None => anyhow::bail!(
                "Trying to append with Command={} without a data attribute",
                self
            ),
        };
        let bytes_appended = data_attribute.append(
            context,
            other_data_attribute,
            context.ssuc_options.maximum_batched_extent_size,
        )?;
        self.sc_uncompressed_size += bytes_appended;
        self.sc_data_attribute_dirty = true;
        self.verify(context)?;
        info!(
            context.ssuc_logger,
            "Appended Command={} with Command={}", self, other
        );
        Ok(bytes_appended)
    }

    pub fn truncate_data_payload_at_start(
        &mut self,
        context: &mut SendStreamUpgradeContext,
        bytes_to_remove: usize,
    ) -> anyhow::Result<()> {
        context.trace_stats();
        info!(
            context.ssuc_logger,
            "Truncating at start Command={} Size={}B", self, bytes_to_remove
        );
        let data_attribute = match &mut self.sc_data_attribute {
            Some(attribute) => attribute,
            None => anyhow::bail!(
                "Trying to truncate Command={} without a data attribute",
                self
            ),
        };
        // Ensure that we have an initial size too
        match self.sc_data_attribute_initial_size {
            None => anyhow::bail!(
                "Trying to truncate Command={} without a data attribute initial size",
                self
            ),
            _ => {}
        }
        let offset = match self.sc_start_offset {
            Some(offset) => offset,
            None => anyhow::bail!("Trying to truncate Command={} without an offset", self),
        };
        data_attribute.truncate_payload_at_start(context, bytes_to_remove)?;
        self.sc_uncompressed_size -= bytes_to_remove;
        self.sc_data_attribute_dirty = true;
        self.sc_start_offset = Some(offset + bytes_to_remove);
        self.verify(context)?;
        info!(
            context.ssuc_logger,
            "Truncated at start Command={} Size={}B", self, bytes_to_remove
        );
        Ok(())
    }

    ///
    /// Generates a pad command for use with clone-on-receive testing
    ///
    /// For the clone-on-receive prototype to work, the following must be true:
    /// * The write command to be cloned must not be encoded
    /// * The size of the data payload of the write command should be aligned to
    /// the page size
    /// * The data payload of the write command should start at a page offset
    /// from the start of the send stream file
    ///
    /// The pad command helps achieve this last requirement.
    ///
    /// For example, say we had the following command:
    ///
    /// |Command Header|Attr0|Attr1|...|Data Attr Header|Data Payload ->|
    ///                                                 ^
    ///                                                 |
    ///                                                 Starts at x % 4096B
    ///                                                 in the
    ///                                                 send stream file
    ///
    /// Adding a "pad command" would help us align the start of the data payload
    /// to a page offset as follows:
    ///
    /// |PAD|Command Header|Attr0|Attr1|...|Data Attr Header|Data Payload ->|
    ///   ^                                                 ^
    ///   |                                                 |
    ///   Sized                                             Now starts at 0 %
    ///   to be 4096B - x                                   4096B in the
    ///                                                     send stream file
    ///
    /// As a result, the data payload region can be directly cloned into a
    /// different file on receive.
    ///
    /// This command is generated with the BTRFS_SEND_C_UPDATE_EXTENT type;
    /// since this command type is currently unused, a padded send stream should
    /// also apply cleanly when using a regular receive command.
    ///
    pub fn generate_pad_command(
        &self,
        context: &mut SendStreamUpgradeContext,
    ) -> anyhow::Result<Self> {
        context.trace_stats();
        let version = context.get_destination_version()?;
        info!(context.ssuc_logger, "Padding Command={}", self);
        anyhow::ensure!(
            !self.sc_data_attribute_dirty,
            "Padding dirty Command={}",
            self
        );
        let data_attribute = self
            .sc_data_attribute
            .as_ref()
            .with_context(|| format!("Trying to pad Command={} without a data attribute", self))?;
        let data_attribute_initial_size =
            self.sc_data_attribute_initial_size.with_context(|| {
                format!(
                    "Trying to pad Command={} without a data attribute initial size",
                    self
                )
            })?;
        let pre_data_attribute_size = self.sc_buffer.len() - data_attribute_initial_size;
        let header_size = SendCommandHeader::get_header_size();
        let path_attribute =
            SendAttribute::new_from_string(context, BtrfsSendAttributeType::BTRFS_SEND_A_PATH, "")?;
        let file_offset_attribute = SendAttribute::new_from_u64(
            context,
            BtrfsSendAttributeType::BTRFS_SEND_A_FILE_OFFSET,
            0,
        )?;
        let size_attribute =
            SendAttribute::new_from_u64(context, BtrfsSendAttributeType::BTRFS_SEND_A_SIZE, 0)?;
        let data_attribute_header_size =
            data_attribute.get_size() - data_attribute.get_payload_size();
        let mut pad_size = BLOCK_SIZE
            - (context.get_write_offset() + pre_data_attribute_size + data_attribute_header_size)
                % BLOCK_SIZE;
        // Account for the minimum size of the command
        let minimum_command_overhead = header_size
            + path_attribute.get_size()
            + file_offset_attribute.get_size()
            + size_attribute.get_size();
        if pad_size < minimum_command_overhead {
            pad_size += BLOCK_SIZE;
        }
        let pad_payload_size = pad_size - header_size;
        let mut header = SendCommandHeader::generate_pad_command_header(context, pad_payload_size)?;
        let mut buffer = vec![0; pad_size];
        // Generate a string -- this will constitute the bulk of the pad
        let mut pad_path = String::new();
        for _i in 0..pad_size {
            pad_path.push('a');
        }
        // Regenerate the path attribute with the new path
        let path_attribute = SendAttribute::new_from_string(
            context,
            BtrfsSendAttributeType::BTRFS_SEND_A_PATH,
            &pad_path,
        )?;

        {
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(&mut buffer), version, version);
            // Skip the CRC update for now
            header.persist(&mut sub_context, true)?;
            path_attribute.persist(&mut sub_context)?;
            file_offset_attribute.persist(&mut sub_context)?;
            size_attribute.persist(&mut sub_context)?;
            context.return_child(&mut sub_context);
        }

        let computed_crc32c = Self::compute_crc32c(context, &buffer);
        header.set_crc32c(computed_crc32c)?;

        {
            // Persist the header
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(&mut buffer), version, version);
            // Flush the CRC32C this time
            header.persist(&mut sub_context, false)?;
            context.return_child(&mut sub_context);
        }

        let command = SendCommand {
            sc_header: header,
            sc_buffer: buffer,
            sc_data_attribute: None,
            sc_data_attribute_initial_size: None,
            sc_data_attribute_dirty: false,
            sc_path: None,
            sc_start_offset: None,
            sc_uncompressed_size: pad_size,
            sc_version: version,
        };
        info!(
            context.ssuc_logger,
            "Padded Command={} with Command{}", self, command
        );
        Ok(command)
    }

    pub fn persist(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<()> {
        context.trace_stats();
        info!(
            context.ssuc_logger,
            "Writing Command={} at offset={}",
            self,
            context.get_write_offset()
        );
        self.verify(context)?;
        let destination_version = context.get_destination_version()?;
        anyhow::ensure!(
            self.sc_version == destination_version,
            "Version mismatch while persisting struct version={} destination version={}",
            self.sc_version,
            destination_version
        );
        // TODO: Allow for a dirty attribute to be flushed before persisting it
        anyhow::ensure!(
            !self.sc_data_attribute_dirty,
            "Writing dirty Command={}",
            self
        );
        if context.ssuc_options.pad_with_dummy_commands && self.sc_header.is_paddable() {
            let data_attribute = match &self.sc_data_attribute {
                Some(attribute) => attribute,
                None => anyhow::bail!("Trying to pad Command={} without a data attribute", self),
            };
            let start_offset = match self.sc_start_offset {
                Some(offset) => offset,
                None => anyhow::bail!("Trying to pad Command={} without a start offset", self),
            };
            // Only payloads that are block-size aligned can be cloned
            // These are the only commands that should be padded
            if start_offset % BLOCK_SIZE == 0 && data_attribute.get_payload_size() % BLOCK_SIZE == 0
            {
                let pad_command = self.generate_pad_command(context)?;
                context.write(&pad_command.sc_buffer, pad_command.sc_uncompressed_size)?;
                let data_attribute_initial_size = match self.sc_data_attribute_initial_size {
                    Some(size) => size,
                    None => anyhow::bail!(
                        "Trying to pad Command={} without a data attribute initial size",
                        self
                    ),
                };
                let pre_data_attribute_size = self.sc_buffer.len() - data_attribute_initial_size;
                let write_offset = context.get_write_offset();
                let data_attribute_header_size =
                    data_attribute.get_size() - data_attribute.get_payload_size();
                let expected_data_payload_offset =
                    write_offset + pre_data_attribute_size + data_attribute_header_size;
                anyhow::ensure!(
                    expected_data_payload_offset % BLOCK_SIZE == 0,
                    "Generated pad Command={}, but write offset is {} and expected data payload offset is {}",
                    pad_command,
                    write_offset,
                    expected_data_payload_offset
                );
                info!(
                    context.ssuc_logger,
                    "Generated pad Command={} for write_offset={} data_payload_offset={}",
                    pad_command,
                    write_offset,
                    expected_data_payload_offset
                );
            }
        }
        context.write(&self.sc_buffer, self.sc_uncompressed_size)
    }

    pub fn is_appendable(&self) -> bool {
        self.sc_header.is_appendable()
    }

    pub fn can_append(&self, other: &Self) -> bool {
        let data_attribute = match &self.sc_data_attribute {
            Some(attribute) => attribute,
            None => return false,
        };
        let start_offset = match self.sc_start_offset {
            Some(offset) => offset,
            None => return false,
        };
        let other_start_offset = match other.sc_start_offset {
            Some(offset) => offset,
            None => return false,
        };
        self.sc_version == other.sc_version
            && self.sc_header.are_commands_appendable(&other.sc_header)
            && self.sc_path == other.sc_path
            && other.sc_data_attribute.is_some()
            && start_offset + data_attribute.get_uncompressed_payload_size() == other_start_offset
    }

    pub fn is_upgradeable(&self, context: &SendStreamUpgradeContext) -> anyhow::Result<bool> {
        self.sc_header.is_command_upgradeable(context)
    }

    pub fn is_compressible(&self) -> bool {
        self.sc_header.is_command_compressible()
    }

    pub fn is_end(&self) -> bool {
        self.sc_header.is_command_end()
    }

    pub fn is_empty(&self) -> bool {
        let data_attribute = match &self.sc_data_attribute {
            Some(attribute) => attribute,
            None => return false,
        };
        data_attribute.get_uncompressed_payload_size() == 0
    }

    pub fn is_full(&self, context: &SendStreamUpgradeContext) -> bool {
        let data_attribute = match &self.sc_data_attribute {
            Some(attribute) => attribute,
            None => return false,
        };
        data_attribute.get_uncompressed_payload_size()
            >= context.ssuc_options.maximum_batched_extent_size
    }

    pub fn is_dirty(&self) -> bool {
        self.sc_data_attribute_dirty
    }

    pub fn get_cached_data_payload_size(&self) -> anyhow::Result<usize> {
        match self.sc_data_attribute {
            Some(ref attribute) => Ok(attribute.get_payload_size()),
            // Nothing cached, so nothing to return
            None => Ok(0),
        }
    }

    fn compute_crc32c(context: &mut SendStreamUpgradeContext, buffer: &[u8]) -> u32 {
        let start_time = SystemTime::now();
        let checksum = crc32c_hw::update(!0, buffer);
        context.update_crc32c_stats(&start_time, buffer.len());
        !checksum
    }

    fn fmt_internal(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<SendCommand Header={} DataAttribute={:?} DataAttributeInitialSize={:?} DataAttributeDirty={} Path={:?} StartOffset={:?} BufferLen={} UncompressedBytes={} Version={}/>",
            self.sc_header,
            self.sc_data_attribute,
            self.sc_data_attribute_initial_size,
            self.sc_data_attribute_dirty,
            self.sc_path,
            self.sc_start_offset,
            self.sc_buffer.len(),
            self.sc_uncompressed_size,
            self.sc_version
        )
    }
}

impl PartialEq for SendCommand {
    fn eq(&self, other: &Self) -> bool {
        // Don't check uncompressed size -- we'll only know this if we've taken a write request and
        // compressed it directly
        if self.sc_header != other.sc_header
            || self.sc_path != other.sc_path
            || self.sc_start_offset != other.sc_start_offset
            || self.sc_version != other.sc_version
        {
            return false;
        }
        // If neither one is dirty, just compare the buffers
        if !self.sc_data_attribute_dirty && !other.sc_data_attribute_dirty {
            self.sc_buffer == other.sc_buffer
        } else {
            // We must have an initial size if the data attribute can be dirtied
            let data_attribute_initial_size = match self.sc_data_attribute_initial_size {
                Some(size) => size,
                None => return false,
            };
            let other_data_attribute_initial_size = match other.sc_data_attribute_initial_size {
                Some(size) => size,
                None => return false,
            };
            let pre_data_attribute_size = self.sc_buffer.len() - data_attribute_initial_size;
            let other_pre_data_attribute_size =
                other.sc_buffer.len() - other_data_attribute_initial_size;
            if pre_data_attribute_size != other_pre_data_attribute_size {
                return false;
            }
            // Finish by comparing the data attributes themselves
            self.sc_data_attribute == other.sc_data_attribute
        }
    }
}
