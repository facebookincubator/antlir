/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;
use std::fmt::Display;
use std::mem::size_of_val;
use std::time::SystemTime;

use slog::debug;
use slog::info;
use slog::trace;
use thiserror::Error;
use zstd::stream::write::Encoder;

use crate::send_elements::send_attribute_header::BtrfsSendAttributeType;
use crate::send_elements::send_attribute_header::SendAttributeHeader;
use crate::send_elements::send_version::SendVersion;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

// This must be set to 17 (i.e., 2 ^ 17 = 128Ki)
const BTRFS_ZSTD_WINDOW_LOG: u32 = 17;

#[derive(Debug, Error)]
#[error(
    "Failed to compress attribute: old payload size {saftspe_old_payload_size}B is smaller than new payload size {saftspe_new_payload_size}B + bytes to save {saftspe_min_bytes_to_save}B"
)]
pub struct SendAttributeFailedToShrinkPayloadError {
    /// The previous size of the attribute payload
    saftspe_old_payload_size: usize,
    /// The proposed size of the attribute payload
    saftspe_new_payload_size: usize,
    /// The minimum number of bytes that needed to have been saved
    saftspe_min_bytes_to_save: usize,
}

pub struct SendAttribute {
    /// The header for the current command
    sa_header: SendAttributeHeader,
    /// A vector containing a buffer for the entire command
    /// (including the header)
    sa_buffer: Vec<u8>,
    /// The uncompressed size of the attribute
    sa_uncompressed_size: usize,
    /// The uncompressed payload size of the attribute
    sa_uncompressed_payload_size: usize,
    /// The version of the attribute
    sa_version: SendVersion,
}

impl Display for SendAttribute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl Debug for SendAttribute {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.fmt_internal(f)
    }
}

impl SendAttribute {
    pub fn new(context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        let version = context.get_source_version()?;
        let bytes_remaining = context.get_read_len()? - context.get_read_offset();
        // Start off by creating the attribute header
        let header = SendAttributeHeader::new(context)?;
        let header_size = header.get_header_size();
        let total_size = header.get_attribute_total_size(context, bytes_remaining)?;
        trace!(
            context.ssuc_logger,
            "New attribute total size {}B header size {}B remaining {}B",
            total_size,
            header_size,
            bytes_remaining
        );
        let start_time = SystemTime::now();
        let mut buffer = vec![0; total_size];
        context.update_attribute_population_stats(&start_time);

        {
            // Persist the header
            // Set up a new sub context on the basis of the local buffer
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(&mut buffer[..]), version, version);
            header.persist(&mut sub_context)?;
            context.return_child(&mut sub_context);
        }

        // Read in the rest of the attribute
        trace!(
            context.ssuc_logger,
            "New attribute read offset {}B read remaining {}B",
            context.get_read_offset(),
            bytes_remaining
        );
        context.read_exact(&mut buffer[header_size..])?;
        let start_time = SystemTime::now();
        let attribute = SendAttribute {
            sa_header: header,
            sa_buffer: buffer,
            sa_uncompressed_size: total_size,
            sa_uncompressed_payload_size: total_size - header_size,
            sa_version: version,
        };
        context.update_attribute_population_stats(&start_time);
        debug!(context.ssuc_logger, "New Attribute={}", attribute);
        let header_slice = &attribute.sa_buffer[..header_size];
        trace!(
            context.ssuc_logger,
            "New AttributeHeader bytes {:02X?}", header_slice
        );
        Ok(attribute)
    }

    pub fn new_from_u32(
        context: &mut SendStreamUpgradeContext,
        attribute_type: BtrfsSendAttributeType,
        value: u32,
    ) -> anyhow::Result<Self> {
        context.trace_stats();
        // Start off by creating the attribute header
        let version = context.get_destination_version()?;
        let payload_size = size_of_val(&value);
        let header = SendAttributeHeader::construct(attribute_type, payload_size as u16, version);
        let header_size = header.get_header_size();
        let total_size = header_size + payload_size;
        let mut buffer = vec![0; total_size];

        {
            // Persist the header
            // Set up a new sub context on the basis of the local buffer
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(&mut buffer[..]), version, version);
            header.persist(&mut sub_context)?;
            sub_context.write32(value)?;
            context.return_child(&mut sub_context);
        }

        let attribute = SendAttribute {
            sa_header: header,
            sa_buffer: buffer,
            sa_uncompressed_size: total_size,
            sa_uncompressed_payload_size: payload_size,
            sa_version: version,
        };
        debug!(context.ssuc_logger, "NewFromInt Attribute={}", attribute);
        let header_slice = &attribute.sa_buffer[..header_size];
        trace!(
            context.ssuc_logger,
            "NewFromInt AttributeHeader bytes {:02X?}", header_slice
        );
        Ok(attribute)
    }

    pub fn new_from_u64(
        context: &mut SendStreamUpgradeContext,
        attribute_type: BtrfsSendAttributeType,
        value: u64,
    ) -> anyhow::Result<Self> {
        context.trace_stats();
        // Start off by creating the attribute header
        let version = context.get_destination_version()?;
        let payload_size = size_of_val(&value);
        let header = SendAttributeHeader::construct(attribute_type, payload_size as u16, version);
        let header_size = header.get_header_size();
        let total_size = header_size + payload_size;
        let mut buffer = vec![0; total_size];

        {
            // Persist the header
            // Set up a new sub context on the basis of the local buffer
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(&mut buffer[..]), version, version);
            header.persist(&mut sub_context)?;
            sub_context.write64(value)?;
            context.return_child(&mut sub_context);
        }

        let attribute = SendAttribute {
            sa_header: header,
            sa_buffer: buffer,
            sa_uncompressed_size: total_size,
            sa_uncompressed_payload_size: payload_size,
            sa_version: version,
        };
        debug!(context.ssuc_logger, "NewFromInt Attribute={}", attribute);
        let header_slice = &attribute.sa_buffer[..header_size];
        trace!(
            context.ssuc_logger,
            "NewFromInt AttributeHeader bytes {:02X?}", header_slice
        );
        Ok(attribute)
    }

    pub fn new_from_string(
        context: &mut SendStreamUpgradeContext,
        attribute_type: BtrfsSendAttributeType,
        string: &str,
    ) -> anyhow::Result<Self> {
        context.trace_stats();
        // Start off by creating the attribute header
        let version = context.get_destination_version()?;
        let string_buffer = string.as_bytes();
        let payload_size = string_buffer.len();
        let header = SendAttributeHeader::construct(attribute_type, payload_size as u16, version);
        let header_size = header.get_header_size();
        let total_size = header_size + payload_size;
        let mut buffer = vec![0; total_size];

        {
            // Persist the header
            // Set up a new sub context on the basis of the local buffer
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(&mut buffer[..]), version, version);
            header.persist(&mut sub_context)?;
            sub_context.write_all(string_buffer, string_buffer.len())?;
            context.return_child(&mut sub_context);
        }

        let attribute = SendAttribute {
            sa_header: header,
            sa_buffer: buffer,
            sa_uncompressed_size: total_size,
            sa_uncompressed_payload_size: payload_size,
            sa_version: version,
        };
        debug!(context.ssuc_logger, "NewFromString Attribute={}", attribute);
        let header_slice = &attribute.sa_buffer[..header_size];
        trace!(
            context.ssuc_logger,
            "NewFromString AttributeHeader bytes {:02X?}", header_slice
        );
        Ok(attribute)
    }

    pub fn copy_range(
        &self,
        context: &mut SendStreamUpgradeContext,
        payload_start_offset: usize,
        payload_end_offset: usize,
    ) -> anyhow::Result<Self> {
        context.trace_stats();
        debug!(
            context.ssuc_logger,
            "Copying range Attribute={} Start={} End={}",
            self,
            payload_start_offset,
            payload_end_offset
        );
        let version = self.sa_version;
        let header = self.sa_header.copy();
        let header_size = header.get_header_size();
        let payload_size = payload_end_offset - payload_start_offset;
        let total_size = header_size + payload_size;
        let start_time = SystemTime::now();
        let mut buffer = vec![0; total_size];
        context.update_attribute_population_stats(&start_time);

        {
            // Set up a new sub context on the basis of the local buffer
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(&mut buffer[..]), version, version);
            // Persist the header
            header.persist(&mut sub_context)?;
            // Now write the data in the given range
            // Be sure to skip the header at the start of the attribute
            let copy_start_offset = payload_start_offset + header_size;
            let copy_end_offset = payload_end_offset + header_size;
            sub_context.write_all(
                &self.sa_buffer[copy_start_offset..copy_end_offset],
                payload_size,
            )?;
            context.return_child(&mut sub_context);
        }

        let start_time = SystemTime::now();
        let attribute = SendAttribute {
            sa_header: header,
            sa_buffer: buffer,
            sa_uncompressed_size: total_size,
            sa_uncompressed_payload_size: payload_size,
            sa_version: version,
        };
        context.update_attribute_population_stats(&start_time);
        debug!(
            context.ssuc_logger,
            "Copied range to Attribute={}", attribute
        );
        let header_slice = &attribute.sa_buffer[..header_size];
        trace!(
            context.ssuc_logger,
            "Copied AttributeHeader bytes {:02X?}", header_slice
        );
        Ok(attribute)
    }

    pub fn upgrade(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<Self> {
        context.trace_stats();
        let version = context.get_destination_version()?;
        debug!(context.ssuc_logger, "Upgrading Attribute={}", self);
        anyhow::ensure!(
            self.is_upgradeable(context)?,
            "Trying to upgrade an unupgradeable attribute"
        );
        let upgraded_header = self.sa_header.upgrade(context)?;
        let old_header_size = self.sa_header.get_header_size();
        let new_header_size = upgraded_header.get_header_size();
        // Set up the header
        let payload_size = self.get_payload_size();
        let mut upgraded_buffer = vec![0; new_header_size + payload_size];

        // Create a new context for persisting the header
        {
            // Persist the header
            let mut sub_context = context.clone_with_new_buffers(
                None,
                Some(&mut upgraded_buffer[..]),
                self.sa_version,
                version,
            );
            upgraded_header.persist(&mut sub_context)?;
            context.return_child(&mut sub_context);
        }

        {
            // Fill up the vector with enough space for the rest of the payload
            let mut sub_context = context.clone_with_new_buffers(
                Some(&self.sa_buffer[old_header_size..]),
                None,
                self.sa_version,
                version,
            );
            sub_context.read_exact(&mut upgraded_buffer[new_header_size..])?;
            context.return_child(&mut sub_context);
        }

        let new_attribute = SendAttribute {
            sa_header: upgraded_header,
            sa_buffer: upgraded_buffer,
            sa_uncompressed_size: new_header_size + payload_size,
            sa_uncompressed_payload_size: payload_size,
            sa_version: version,
        };
        debug!(
            context.ssuc_logger,
            "Upgraded NewAttribute={}", new_attribute
        );
        Ok(new_attribute)
    }

    pub fn fake_an_upgrade(&mut self, context: &SendStreamUpgradeContext) -> anyhow::Result<()> {
        anyhow::ensure!(
            !self.is_upgradeable(context)?,
            "Trying to fake upgrade an upgradeable Attribute={}",
            self
        );
        self.sa_version = context.get_destination_version()?;
        Ok(())
    }

    pub fn compress(
        &self,
        context: &mut SendStreamUpgradeContext,
        minimum_bytes_to_save: usize,
    ) -> anyhow::Result<Self> {
        context.trace_stats();
        let version = context.get_destination_version()?;
        debug!(
            context.ssuc_logger,
            "Compressing Attribute={} must_save={}", self, minimum_bytes_to_save
        );
        // Based on how this code is structured, an attribute must be upgraded before
        // undergoing compression
        anyhow::ensure!(
            !self.is_upgradeable(context)? && self.is_compressible(),
            "Trying to compress an unupgraded or uncompressible attribute"
        );
        let compressed_header = self.sa_header.compress(context)?;
        let old_header_size = self.sa_header.get_header_size();
        let new_header_size = compressed_header.get_header_size();
        // Set up the header
        let mut compressed_buffer = vec![0; new_header_size];
        let old_payload_size = self.get_payload_size();

        // Create a new context for persisting the header
        {
            let mut sub_context = context.clone_with_new_buffers(
                None,
                Some(&mut compressed_buffer[..]),
                self.sa_version,
                version,
            );
            // Persist the header
            compressed_header.persist(&mut sub_context)?;
            context.return_child(&mut sub_context);
        }

        anyhow::ensure!(
            context.ssuc_options.compression_level != 0,
            "Compressing with no compression level set"
        );
        let start_time = SystemTime::now();
        let mut encoder = Encoder::new(
            &mut compressed_buffer,
            context.ssuc_options.compression_level,
        )?;
        encoder.window_log(BTRFS_ZSTD_WINDOW_LOG)?;
        std::io::copy(&mut &self.sa_buffer[old_header_size..], &mut encoder)?;
        encoder.finish()?;
        let new_attribute = SendAttribute {
            sa_header: compressed_header,
            sa_buffer: compressed_buffer,
            sa_uncompressed_size: self.sa_uncompressed_size,
            sa_uncompressed_payload_size: old_payload_size,
            sa_version: version,
        };
        let new_payload_size = new_attribute.get_payload_size();
        context.update_compress_stats(
            &start_time,
            old_payload_size >= new_payload_size + minimum_bytes_to_save,
        );
        debug!(
            context.ssuc_logger,
            "Compressed NewAttribute={} OldPayloadSize={}B NewPayloadSize={}B MinToSave={}B",
            new_attribute,
            old_payload_size,
            new_payload_size,
            minimum_bytes_to_save
        );
        // Ensure that we save a minimum number of bytes
        anyhow::ensure!(
            old_payload_size >= new_payload_size + minimum_bytes_to_save,
            SendAttributeFailedToShrinkPayloadError {
                saftspe_old_payload_size: old_payload_size,
                saftspe_new_payload_size: new_payload_size,
                saftspe_min_bytes_to_save: minimum_bytes_to_save
            }
        );
        Ok(new_attribute)
    }

    pub fn compress_space_check(&self, context: &SendStreamUpgradeContext) -> anyhow::Result<()> {
        // Find out how much space is left
        let write_offset = context.get_write_offset();
        let write_length = context.get_write_len()?;
        let bytes_remaining = write_length - write_offset;
        // Ensure the given context can accommodate this attribute
        anyhow::ensure!(
            bytes_remaining >= self.sa_buffer.len(),
            SendAttributeFailedToShrinkPayloadError {
                saftspe_old_payload_size: write_length,
                saftspe_new_payload_size: write_offset + self.sa_buffer.len(),
                saftspe_min_bytes_to_save: 0
            }
        );
        Ok(())
    }

    pub fn append(
        &mut self,
        context: &mut SendStreamUpgradeContext,
        other: &Self,
        max_payload_size: usize,
    ) -> anyhow::Result<usize> {
        context.trace_stats();
        debug!(
            context.ssuc_logger,
            "Appending Attribute={} with Attribute={} MaxPayloadSize={}",
            self,
            other,
            max_payload_size
        );
        // Exit before any potential overflow
        if max_payload_size < self.sa_uncompressed_payload_size {
            return Ok(0);
        }
        anyhow::ensure!(
            self.can_append(other),
            "Appending unappendable Attribute={} with Attribute={}",
            self,
            other
        );
        // TODO: Maybe add this feature in the future?
        anyhow::ensure!(
            !self.sa_header.has_size(),
            "No support for updating size for v1 streams as a part of append"
        );
        // Cap the number of bytes to append
        let bytes_to_append = std::cmp::min(
            other.sa_uncompressed_payload_size,
            max_payload_size - self.sa_uncompressed_payload_size,
        );
        // Determine the header position of the other command
        let other_buffer_start_offset = other.sa_header.get_header_size();
        // Determine the end of the number of bytes to copy
        let other_buffer_end_offset = other_buffer_start_offset + bytes_to_append;
        // Extend the buffer based on the number of bytes to append
        debug!(
            context.ssuc_logger,
            "Appending to Attribute={} with sizes OtherPayloadBytes={} MaxPayloadBytes={} BytesToAppend={} CopyStartOffset={} CopyEndOffset={}",
            self,
            other.sa_uncompressed_payload_size,
            max_payload_size,
            bytes_to_append,
            other_buffer_start_offset,
            other_buffer_end_offset
        );
        let start_time = SystemTime::now();
        self.sa_buffer.extend_from_slice(
            &other.sa_buffer[other_buffer_start_offset..other_buffer_end_offset],
        );
        context.update_append_stats(&start_time, bytes_to_append);
        // Adjust sizes
        self.sa_uncompressed_size += bytes_to_append;
        self.sa_uncompressed_payload_size += bytes_to_append;
        debug!(
            context.ssuc_logger,
            "Appended Attribute={} with Attribute={}", self, other
        );
        // Return the number of bytes appended
        Ok(bytes_to_append)
    }

    pub fn truncate_payload_at_start(
        &mut self,
        context: &mut SendStreamUpgradeContext,
        bytes_to_remove: usize,
    ) -> anyhow::Result<()> {
        context.trace_stats();
        debug!(
            context.ssuc_logger,
            "Truncating {}B from payload at start for Attribute={}", bytes_to_remove, self
        );
        if bytes_to_remove == 0 {
            return Ok(());
        }
        anyhow::ensure!(
            self.is_truncateable(),
            "Truncating untruncatable Attribute={}",
            self
        );
        anyhow::ensure!(
            bytes_to_remove <= self.sa_uncompressed_payload_size,
            "Not enough data {}B to truncate from Attribute={}",
            bytes_to_remove,
            self
        );
        // TODO: Maybe add this feature in the future?
        anyhow::ensure!(
            !self.sa_header.has_size(),
            "No support for updating size for v1 streams as a part of truncate"
        );
        // First determine the header position
        let payload_offset = self.sa_header.get_header_size();
        // Figure out how many bytes we want to keep in our buffer
        let bytes_to_preserve = self.sa_uncompressed_payload_size - bytes_to_remove;
        // Copy whatever must be preserved
        debug!(
            context.ssuc_logger,
            "Truncating Attribute={} with sizes BytesToRemove={} BytesToPreserve={} Start={}",
            self,
            bytes_to_remove,
            bytes_to_preserve,
            payload_offset
        );
        let start_time = SystemTime::now();
        self.sa_buffer.copy_within(
            payload_offset + bytes_to_remove..payload_offset + bytes_to_remove + bytes_to_preserve,
            payload_offset,
        );
        // Truncate as appropriate
        self.sa_buffer.truncate(bytes_to_preserve + payload_offset);
        context.update_truncate_stats(&start_time, bytes_to_remove);
        // Adjust sizes
        self.sa_uncompressed_size -= bytes_to_remove;
        self.sa_uncompressed_payload_size -= bytes_to_remove;
        debug!(
            context.ssuc_logger,
            "Truncated {}B from payload at start for Attribute={}", bytes_to_remove, self
        );
        Ok(())
    }

    pub fn verify(&mut self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<()> {
        if !context.ssuc_options.serde_checks {
            return Ok(());
        }
        context.trace_stats();
        let version = self.sa_version;
        info!(context.ssuc_logger, "Checking Attribute={}", self);
        let header_slice = &self.sa_buffer[..self.sa_header.get_header_size()];
        trace!(
            context.ssuc_logger,
            "Checking AttributeHeader bytes {:02X?}", header_slice
        );
        // Start off by creating the attribute header
        let header;

        // Create a new context for reading the header
        {
            let mut sub_context =
                context.clone_with_new_buffers(Some(&self.sa_buffer[..]), None, version, version);
            header = SendAttributeHeader::new(&mut sub_context)?;
            context.return_child(&mut sub_context);
        }

        trace!(context.ssuc_logger, "Read AttributeHeader={}", header);
        let header_size = header.get_header_size();
        let total_size = header.get_attribute_total_size(context, self.sa_buffer.len())?;
        trace!(
            context.ssuc_logger,
            "Checking attribute total size {}B header size {}B remaining {}B",
            total_size,
            header_size,
            self.sa_buffer.len()
        );
        // First check -- ensure that the buffer size is correct
        anyhow::ensure!(
            total_size == self.sa_buffer.len(),
            "Verifying attribute failed; {}B size doesn't match {}B buffer size",
            total_size,
            self.sa_buffer.len()
        );
        let old_total_size = self
            .sa_header
            .get_attribute_total_size(context, total_size)?;
        // Second check -- the two command sizes must match
        anyhow::ensure!(
            total_size == old_total_size,
            "Verifying command failed; {}B after, {}B before",
            total_size,
            old_total_size
        );
        let mut buffer = vec![0; total_size];

        // Create a new context for persisting the header
        {
            let mut sub_context =
                context.clone_with_new_buffers(None, Some(&mut buffer[..]), version, version);
            // Persist the header
            header.persist(&mut sub_context)?;
            context.return_child(&mut sub_context);
        }

        // Copy over the rest of the data into the buffer
        let start_time = SystemTime::now();
        std::io::copy(
            &mut &self.sa_buffer[header_size..],
            &mut &mut buffer[header_size..],
        )?;
        context.update_copy_stats(&start_time, buffer.len() - header_size);
        let new_attribute = SendAttribute {
            sa_header: header,
            sa_buffer: buffer,
            sa_uncompressed_size: total_size,
            sa_uncompressed_payload_size: total_size - header_size,
            sa_version: version,
        };
        // Third check -- the attributes must match
        anyhow::ensure!(
            *self == new_attribute,
            "Verifying attribute failed; {} attribute != {} reconstructed attribute",
            *self,
            new_attribute
        );
        info!(context.ssuc_logger, "Passed Check on Attribute={}", self);
        Ok(())
    }

    pub fn persist(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<()> {
        context.trace_stats();
        debug!(context.ssuc_logger, "Writing Attribute={}", self);
        let destination_version = context.get_destination_version()?;
        anyhow::ensure!(
            self.sa_version == destination_version,
            "Version mismatch while persisting struct version={} destination version={}",
            self.sa_version,
            destination_version
        );
        context.write_all(&self.sa_buffer, self.sa_uncompressed_size)
    }

    pub fn get_size(&self) -> usize {
        self.sa_buffer.len()
    }

    pub fn get_payload_size(&self) -> usize {
        self.get_size() - self.sa_header.get_header_size()
    }

    pub fn is_upgradeable(&self, context: &SendStreamUpgradeContext) -> anyhow::Result<bool> {
        self.sa_header.is_attribute_upgradeable(context)
    }

    pub fn is_compressible(&self) -> bool {
        self.sa_header.is_attribute_compressible()
    }

    pub fn can_append(&self, other: &Self) -> bool {
        self.sa_version == other.sa_version
            && self.sa_header.can_append_attributes(&other.sa_header)
    }

    pub fn is_truncateable(&self) -> bool {
        self.sa_header.is_attribute_truncatable()
    }

    pub fn is_send_a_path(&self) -> bool {
        self.sa_header.is_attribute_send_a_path()
    }

    pub fn is_send_a_file_offset(&self) -> bool {
        self.sa_header.is_attribute_send_a_file_offset()
    }

    pub fn is_send_a_data(&self) -> bool {
        self.sa_header.is_attribute_send_a_data()
    }

    pub fn get_uncompressed_payload_size(&self) -> usize {
        self.sa_uncompressed_payload_size
    }

    pub fn get_payload_as_string(&self) -> anyhow::Result<String> {
        let header_size = self.sa_header.get_header_size();
        let payload_bytes = &self.sa_buffer[header_size..];
        match std::str::from_utf8(payload_bytes) {
            Ok(s) => Ok(String::from(s)),
            Err(e) => anyhow::bail!(e),
        }
    }

    pub fn get_payload_as_u64(&self) -> anyhow::Result<u64> {
        let header_size = self.sa_header.get_header_size();
        let payload_bytes = &self.sa_buffer[header_size..];
        let byte_array: [u8; 8] = match payload_bytes.try_into() {
            Ok(ba) => ba,
            Err(e) => anyhow::bail!(e),
        };
        Ok(u64::from_le_bytes(byte_array))
    }

    fn fmt_internal(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "<SendAttribute Header={} BufferBytes={} UncompressedBytes={} UncompressedPayloadSize={} Version={}/>",
            self.sa_header,
            self.sa_buffer.len(),
            self.sa_uncompressed_size,
            self.sa_uncompressed_payload_size,
            self.sa_version
        )
    }
}

impl PartialEq for SendAttribute {
    fn eq(&self, other: &Self) -> bool {
        if self.sa_header != other.sa_header || self.sa_version != other.sa_version {
            return false;
        }
        // Don't check the uncompressed size -- just check the buffer contents instead
        self.sa_buffer == other.sa_buffer
    }
}
