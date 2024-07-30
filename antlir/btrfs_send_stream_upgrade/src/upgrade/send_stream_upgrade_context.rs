/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::backtrace::Backtrace;
use std::mem;
use std::time::Duration;
use std::time::SystemTime;

use slog::crit;
use slog::o;
use slog::trace;
use slog::Drain;
use slog::Level;
use slog::Logger;

use crate::mp::sync::sync_container::SyncContainer;
use crate::send_elements::send_version::SendVersion;
use crate::upgrade::send_stream_upgrade_destination::SendStreamUpgradeDestination;
use crate::upgrade::send_stream_upgrade_options::SendStreamUpgradeOptions;
use crate::upgrade::send_stream_upgrade_source::SendStreamUpgradeSource;
use crate::upgrade::send_stream_upgrade_stats::SendStreamUpgradeStats;

pub struct SendStreamUpgradeContext<'a> {
    /// Stats related to the current upgrade context
    pub ssuc_stats: SendStreamUpgradeStats,
    /// Logger associated with the current context
    pub ssuc_logger: Logger,
    /// Options that dicatate how the stream will be upgraded
    pub ssuc_options: SendStreamUpgradeOptions,
    /// Source for the IO context
    ssuc_source: SendStreamUpgradeSource<'a>,
    /// Destination for the IO context
    ssuc_destination: SendStreamUpgradeDestination<'a>,
    /// Check to see if this has a parent associated with it
    ssuc_associated_with_parent: bool,
    /// Backtrace of where the context was allocated
    ssuc_backtrace: Backtrace,
    /// An mp-safe container for synchronization primitives
    pub ssuc_sync_container: Option<SyncContainer>,
}

impl<'a> SendStreamUpgradeContext<'a> {
    pub fn new(options: SendStreamUpgradeOptions) -> anyhow::Result<SendStreamUpgradeContext<'a>> {
        // A verbosity of zero disbles logging
        let logger = if options.verbose == 0 || options.quiet {
            let drain = slog::Discard;
            slog::Logger::root(drain, o!())
        } else {
            // Convert verbosity to a log level
            // Note that extra verbose flags will result in Level::Trace
            let level = match Level::from_usize(options.verbose) {
                Some(log_level) => log_level,
                None => slog::Level::Trace,
            };
            let stderr_term = slog_term::PlainSyncDecorator::new(std::io::stderr());
            let drain = slog_term::FullFormat::new(stderr_term).build().fuse();
            let drain = slog::LevelFilter::new(drain, level).fuse();
            slog::Logger::root(drain, o!())
        };
        // Dump all of the options that we were provided
        trace!(logger, "Input parameters are: {:?}", options);
        let input_file = options.input.clone();
        let read_buffer_size = options.read_buffer_size;
        let output_file = options.output.clone();
        let write_buffer_size = options.write_buffer_size;
        // Don't buffer stdin if we are running in mp mode
        let skip_buffering_for_stdin = options.thread_count != 1;
        Ok(SendStreamUpgradeContext {
            ssuc_stats: SendStreamUpgradeStats::new(),
            ssuc_logger: logger,
            ssuc_options: options,
            ssuc_source: SendStreamUpgradeSource::new_from_file(
                input_file,
                read_buffer_size,
                skip_buffering_for_stdin,
                0,
                None,
            )?,
            ssuc_destination: SendStreamUpgradeDestination::new_from_file(
                output_file,
                write_buffer_size,
                0,
                None,
            )?,
            ssuc_associated_with_parent: false,
            ssuc_backtrace: Backtrace::capture(),
            ssuc_sync_container: None,
        })
    }

    pub fn clone_with_new_buffers(
        &self,
        read: Option<&'a [u8]>,
        write: Option<&'a mut [u8]>,
        source_version: SendVersion,
        destination_version: SendVersion,
    ) -> SendStreamUpgradeContext<'a> {
        let start_time = SystemTime::now();
        let mut new_context = SendStreamUpgradeContext {
            ssuc_stats: self.ssuc_stats,
            ssuc_logger: self.ssuc_logger.clone(),
            ssuc_options: self.ssuc_options.clone(),
            ssuc_source: SendStreamUpgradeSource::new_from_slice(read, 0, Some(source_version)),
            ssuc_destination: SendStreamUpgradeDestination::new_from_slice(
                write,
                0,
                Some(destination_version),
            ),
            ssuc_associated_with_parent: true,
            ssuc_backtrace: Backtrace::capture(),
            ssuc_sync_container: None,
        };
        new_context.ssuc_stats.ssus_context_create_time += Self::get_time_delta(&start_time);
        new_context
    }

    pub fn clone_for_mp_threads(
        keep_source: bool,
        keep_destination: bool,
        logger: Logger,
        options: SendStreamUpgradeOptions,
        source_version: SendVersion,
        destination_version: SendVersion,
        source_offset: usize,
        destination_offset: usize,
        mut sync_container: Option<SyncContainer>,
    ) -> anyhow::Result<SendStreamUpgradeContext<'a>> {
        // Open up the input file
        let source = match keep_source {
            true => {
                let input_file = options.input.clone();
                let read_buffer_size = options.read_buffer_size;
                SendStreamUpgradeSource::new_from_file(
                    input_file,
                    read_buffer_size,
                    false,
                    source_offset,
                    Some(source_version),
                )?
            }
            false => {
                // Detatch the container from the buffer cache
                let buffer_cache = match sync_container {
                    Some(ref mut sync_container) => match sync_container.take_buffer_cache() {
                        Some(buffer_cache) => buffer_cache,
                        None => anyhow::bail!("Mp context with None buffer cache"),
                    },
                    None => anyhow::bail!("Mp context with None container"),
                };
                SendStreamUpgradeSource::new_from_buffer_cache(
                    buffer_cache,
                    source_offset,
                    Some(source_version),
                )?
            }
        };
        // Open up the output file
        let destination = match keep_destination {
            true => {
                let output_file = options.output.clone();
                let write_buffer_size = options.write_buffer_size;
                SendStreamUpgradeDestination::new_from_file(
                    output_file,
                    write_buffer_size,
                    destination_offset,
                    Some(destination_version),
                )?
            }
            false => SendStreamUpgradeDestination::new_from_none(Some(destination_version))?,
        };

        Ok(SendStreamUpgradeContext {
            ssuc_stats: SendStreamUpgradeStats::new(),
            ssuc_logger: logger,
            ssuc_options: options,
            ssuc_source: source,
            ssuc_destination: destination,
            ssuc_associated_with_parent: false,
            ssuc_backtrace: Backtrace::capture(),
            ssuc_sync_container: sync_container,
        })
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> anyhow::Result<usize> {
        let start_time = SystemTime::now();
        let total_bytes_read = self.ssuc_source.read(buffer)?;
        let time_delta = match start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        // Increment the appropriate stats if this was an external io
        if self.ssuc_source.is_external() {
            self.ssuc_stats.ssus_reads_issued += 1;
            self.ssuc_stats.ssus_bytes_read += total_bytes_read;
            self.ssuc_stats.ssus_storage_read_time += time_delta;
        } else {
            self.ssuc_stats.ssus_buffer_read_time += time_delta;
        }
        Ok(total_bytes_read)
    }

    pub fn read_exact(&mut self, buffer: &mut [u8]) -> anyhow::Result<()> {
        let bytes_read = self.read(buffer)?;
        // Ensure that we read exactly what we asked for
        anyhow::ensure!(
            buffer.len() == bytes_read,
            "Failed to read {} bytes instead read {} bytes",
            buffer.len(),
            bytes_read
        );
        Ok(())
    }

    pub fn read16(&mut self) -> anyhow::Result<u16> {
        let value: u16 = 0;
        let mut buffer = value.to_le_bytes();
        self.read_exact(&mut buffer)?;
        Ok(<u16>::from_le_bytes(buffer))
    }

    pub fn read32(&mut self) -> anyhow::Result<u32> {
        let value: u32 = 0;
        let mut buffer = value.to_le_bytes();
        self.read_exact(&mut buffer)?;
        Ok(<u32>::from_le_bytes(buffer))
    }

    pub fn get_read_offset(&self) -> usize {
        self.ssuc_source.get_offset()
    }

    pub fn adjust_read_offset(&mut self, increment: usize) -> anyhow::Result<()> {
        self.ssuc_source.adjust_offset(increment)
    }

    pub fn set_read_offset(&mut self, new_offset: usize) -> anyhow::Result<()> {
        self.ssuc_source.set_offset(new_offset)
    }

    pub fn get_read_len(&self) -> anyhow::Result<usize> {
        self.ssuc_source.get_length()
    }

    pub fn write_all(&mut self, buffer: &[u8], uncompressed_bytes: usize) -> anyhow::Result<()> {
        let start_time = SystemTime::now();
        self.ssuc_destination.write_all(buffer)?;
        let time_delta = match start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        // Increment the appropriate stats
        // Children are built on buffers and only do copies
        if !self.ssuc_associated_with_parent {
            if buffer.len() == uncompressed_bytes {
                self.ssuc_stats.ssus_uncompressed_writes_issued += 1;
                self.ssuc_stats.ssus_uncompressed_bytes_written += buffer.len();
            } else {
                anyhow::ensure!(
                    buffer.len() < uncompressed_bytes,
                    "Wrote {}B but had {}B of uncompressed data",
                    buffer.len(),
                    uncompressed_bytes
                );
                self.ssuc_stats.ssus_compressed_writes_issued += 1;
                self.ssuc_stats.ssus_compressed_bytes_written += buffer.len();
            }
            self.ssuc_stats.ssus_storage_write_time += time_delta;
            self.ssuc_stats.ssus_logical_bytes_written += uncompressed_bytes;
        } else {
            self.ssuc_stats.ssus_buffer_write_time += time_delta;
            self.ssuc_stats.ssus_bytes_copied += buffer.len();
        }
        Ok(())
    }

    pub fn write16(&mut self, value: u16) -> anyhow::Result<()> {
        let buffer = value.to_le_bytes();
        self.write_all(&buffer, mem::size_of_val(&value))?;
        Ok(())
    }

    pub fn write32(&mut self, value: u32) -> anyhow::Result<()> {
        let buffer = value.to_le_bytes();
        self.write_all(&buffer, mem::size_of_val(&value))?;
        Ok(())
    }

    pub fn write64(&mut self, value: u64) -> anyhow::Result<()> {
        let buffer = value.to_le_bytes();
        self.write_all(&buffer, mem::size_of_val(&value))?;
        Ok(())
    }

    pub fn get_write_offset(&self) -> usize {
        self.ssuc_destination.get_offset()
    }

    pub fn get_write_len(&self) -> anyhow::Result<usize> {
        self.ssuc_destination.get_length()
    }

    pub fn update_copy_stats(&mut self, start_time: &SystemTime, bytes_copied: usize) {
        let time_delta = match start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        self.ssuc_stats.ssus_buffer_write_time += time_delta;
        self.ssuc_stats.ssus_bytes_copied += bytes_copied;
    }

    pub fn update_crc32c_stats(&mut self, start_time: &SystemTime, crc32c_bytes: usize) {
        let time_delta = match start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        self.ssuc_stats.ssus_crc32c_time += time_delta;
        self.ssuc_stats.ssus_crc32c_bytes_processed += crc32c_bytes;
    }

    fn get_time_delta(start_time: &SystemTime) -> Duration {
        match start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        }
    }

    pub fn update_compress_stats(&mut self, start_time: &SystemTime, compress_succeeded: bool) {
        self.ssuc_stats.ssus_compress_time += Self::get_time_delta(start_time);
        if compress_succeeded {
            self.ssuc_stats.ssus_compression_passed += 1;
        } else {
            self.ssuc_stats.ssus_compression_failed += 1;
        }
    }

    pub fn update_append_stats(&mut self, start_time: &SystemTime, bytes_appended: usize) {
        self.ssuc_stats.ssus_append_time += Self::get_time_delta(start_time);
        self.ssuc_stats.ssus_appended_bytes += bytes_appended;
    }

    pub fn update_truncate_stats(&mut self, start_time: &SystemTime, bytes_removed: usize) {
        self.ssuc_stats.ssus_truncate_time += Self::get_time_delta(start_time);
        self.ssuc_stats.ssus_truncated_bytes += bytes_removed;
    }

    pub fn update_attribute_population_stats(&mut self, start_time: &SystemTime) {
        self.ssuc_stats.ssus_attribute_population_time += Self::get_time_delta(start_time);
    }

    pub fn eprint_summary_stats(&mut self) -> anyhow::Result<()> {
        if !self.ssuc_options.quiet {
            self.ssuc_stats.eprint_summary_stats()?;
        }
        Ok(())
    }

    pub fn trace_stats(&self) {
        trace!(self.ssuc_logger, "CtxtStats={}", self.ssuc_stats);
    }

    pub fn value_to_version(value: u32) -> anyhow::Result<SendVersion> {
        match u32::try_into(value) {
            Ok(version) => Ok(version),
            Err(error) => anyhow::bail!(error),
        }
    }

    pub fn version_to_value(version: SendVersion) -> u32 {
        version as u32
    }

    pub fn set_versions(&mut self, source_version: SendVersion, destination_version: SendVersion) {
        self.ssuc_source.set_version(source_version);
        self.ssuc_destination.set_version(destination_version);
    }

    pub fn get_source_version(&self) -> anyhow::Result<SendVersion> {
        self.ssuc_source.get_version()
    }

    pub fn get_destination_version(&self) -> anyhow::Result<SendVersion> {
        self.ssuc_destination.get_version()
    }

    // Note: For lifetime reasons, we cannot use Self here and we need to use
    // the underlying type instead
    pub fn return_child(&mut self, child: &mut SendStreamUpgradeContext) {
        let start_time = SystemTime::now();
        self.ssuc_stats = child.ssuc_stats;
        // Returning this to its parent
        // Ensure that it hasn't been freed already
        if !child.ssuc_associated_with_parent {
            panic!("Returning freed context");
        }
        child.ssuc_associated_with_parent = false;
        trace!(
            self.ssuc_logger,
            "Ctxt associated with parent is {}", child.ssuc_associated_with_parent
        );
        self.ssuc_stats.ssus_context_return_time += Self::get_time_delta(&start_time);
    }

    pub fn setup_sync_container(&mut self) -> anyhow::Result<()> {
        self.ssuc_sync_container = Some(SyncContainer::new(&self.ssuc_options)?);
        Ok(())
    }

    pub fn flush(&mut self) -> anyhow::Result<()> {
        self.ssuc_destination.flush()
    }
}

impl<'a> Drop for SendStreamUpgradeContext<'a> {
    fn drop(&mut self) {
        // Ensure that any parents have been disassociated with their
        // children
        if self.ssuc_associated_with_parent {
            crit!(
                self.ssuc_logger,
                "Dangling context found; bt for context is {:?}",
                self.ssuc_backtrace
            );
        }
    }
}
