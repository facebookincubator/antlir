/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use slog::crit;
use slog::o;
use slog::trace;
use slog::Drain;
use slog::Level;
use slog::Logger;
use std::backtrace::Backtrace;
use std::fs::OpenOptions;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Write;
use std::mem;
use std::time::Duration;
use std::time::SystemTime;

pub use crate::send_stream_upgrade_options::SendStreamUpgradeOptions;
pub use crate::send_stream_upgrade_stats::SendStreamUpgradeStats;
pub use crate::send_version::SendVersion;

pub struct SendStreamUpgradeContext<'a> {
    /// Stats related to the current upgrade context
    ssuc_stats: SendStreamUpgradeStats,
    /// Logger associated with the current context
    pub ssuc_logger: Logger,
    /// Options that dicatate how the stream will be upgraded
    pub ssuc_options: SendStreamUpgradeOptions,
    /// Source for the IO context
    ssuc_source: BufReader<Box<dyn Read + 'a>>,
    /// Destination for the IO context
    ssuc_destination: BufWriter<Box<dyn Write + 'a>>,
    /// Version of the stream at the source
    ssuc_source_version: Option<SendVersion>,
    /// Version of the stream at the destination
    ssuc_destination_version: Option<SendVersion>,
    /// Offset into the source
    ssuc_source_offset: usize,
    /// Offset into the destination
    ssuc_destination_offset: usize,
    /// Length of the source
    ssuc_source_length: Option<usize>,
    /// Length of the destination
    ssuc_destination_length: Option<usize>,
    /// Check to see if this has a parent associated with it
    ssuc_associated_with_parent: bool,
    /// Backtrace of where the context was allocated
    ssuc_backtrace: Backtrace,
}

impl<'a> SendStreamUpgradeContext<'a> {
    pub fn new(options: SendStreamUpgradeOptions) -> anyhow::Result<SendStreamUpgradeContext<'a>> {
        // Open up the input file
        let input_file: Box<dyn Read> = match &options.input {
            None => Box::new(std::io::stdin()),
            Some(value) => Box::new(OpenOptions::new().read(true).open(value)?),
        };
        // Open up the output file
        let output_file: Box<dyn Write> = match &options.output {
            None => Box::new(std::io::stdout()),
            Some(value) => Box::new(
                OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(value)?,
            ),
        };
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
        let read = input_file;
        let write = output_file;
        let read_box = Box::new(read);
        let write_box = Box::new(write);
        let read_buffer_size = options.read_buffer_size;
        let write_buffer_size = options.write_buffer_size;
        Ok(SendStreamUpgradeContext {
            ssuc_stats: SendStreamUpgradeStats::new(),
            ssuc_logger: logger,
            ssuc_options: options,
            ssuc_source: BufReader::with_capacity(read_buffer_size, read_box as _),
            ssuc_destination: BufWriter::with_capacity(write_buffer_size, write_box as _),
            ssuc_source_version: None,
            ssuc_destination_version: None,
            ssuc_source_offset: 0,
            ssuc_destination_offset: 0,
            ssuc_source_length: None,
            ssuc_destination_length: None,
            ssuc_associated_with_parent: false,
            ssuc_backtrace: Backtrace::capture(),
        })
    }

    pub fn clone_with_new_buffers(
        &self,
        read: &'a [u8],
        write: &'a mut [u8],
        source_version: SendVersion,
        destination_version: SendVersion,
    ) -> SendStreamUpgradeContext<'a> {
        let read_len = read.len();
        let write_len = write.len();
        let read_box = Box::new(read);
        let write_box = Box::new(write);
        SendStreamUpgradeContext {
            ssuc_stats: self.ssuc_stats,
            ssuc_logger: self.ssuc_logger.clone(),
            ssuc_options: self.ssuc_options.clone(),
            ssuc_source: BufReader::new(read_box as _),
            ssuc_destination: BufWriter::new(write_box as _),
            ssuc_source_version: Some(source_version),
            ssuc_destination_version: Some(destination_version),
            ssuc_source_offset: 0,
            ssuc_destination_offset: 0,
            ssuc_source_length: Some(read_len),
            ssuc_destination_length: Some(write_len),
            ssuc_associated_with_parent: true,
            ssuc_backtrace: Backtrace::capture(),
        }
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> anyhow::Result<()> {
        let start_time = SystemTime::now();
        self.ssuc_source.read_exact(buffer)?;
        let time_delta = match start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        // Increment the appropriate stats if this is not a child
        // since children are not associated with actual file io
        if !self.ssuc_associated_with_parent {
            self.ssuc_stats.ssus_reads_issued += 1;
            self.ssuc_stats.ssus_bytes_read += buffer.len();
            self.ssuc_stats.ssus_storage_read_time += time_delta;
        } else {
            self.ssuc_stats.ssus_buffer_read_time += time_delta;
        }
        self.ssuc_source_offset += buffer.len();
        Ok(())
    }

    pub fn read16(&mut self) -> anyhow::Result<u16> {
        let value: u16 = 0;
        let mut buffer = value.to_le_bytes();
        self.read(&mut buffer)?;
        Ok(<u16>::from_le_bytes(buffer))
    }

    pub fn read32(&mut self) -> anyhow::Result<u32> {
        let value: u32 = 0;
        let mut buffer = value.to_le_bytes();
        self.read(&mut buffer)?;
        Ok(<u32>::from_le_bytes(buffer))
    }

    pub fn get_read_offset(&self) -> usize {
        self.ssuc_source_offset
    }

    pub fn get_read_len(&self) -> anyhow::Result<usize> {
        match self.ssuc_source_length {
            Some(length) => Ok(length),
            None => anyhow::bail!("Attempted to get a length of a non-buffer source!"),
        }
    }

    pub fn write(&mut self, buffer: &[u8], uncompressed_bytes: usize) -> anyhow::Result<()> {
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
        self.ssuc_destination_offset += buffer.len();
        Ok(())
    }

    pub fn write16(&mut self, value: u16) -> anyhow::Result<()> {
        let buffer = value.to_le_bytes();
        self.write(&buffer, mem::size_of_val(&value))?;
        Ok(())
    }

    pub fn write32(&mut self, value: u32) -> anyhow::Result<()> {
        let buffer = value.to_le_bytes();
        self.write(&buffer, mem::size_of_val(&value))?;
        Ok(())
    }

    pub fn write64(&mut self, value: u64) -> anyhow::Result<()> {
        let buffer = value.to_le_bytes();
        self.write(&buffer, mem::size_of_val(&value))?;
        Ok(())
    }

    pub fn get_write_offset(&self) -> usize {
        self.ssuc_destination_offset
    }

    pub fn get_write_len(&self) -> anyhow::Result<usize> {
        match self.ssuc_destination_length {
            Some(length) => Ok(length),
            None => anyhow::bail!("Attempted to get a length of a non-buffer destination!"),
        }
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

    pub fn update_compress_stats(&mut self, start_time: &SystemTime, compress_succeeded: bool) {
        let time_delta = match start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        self.ssuc_stats.ssus_compress_time += time_delta;
        if compress_succeeded {
            self.ssuc_stats.ssus_compression_passed += 1;
        } else {
            self.ssuc_stats.ssus_compression_failed += 1;
        }
    }

    pub fn update_append_stats(&mut self, start_time: &SystemTime, bytes_appended: usize) {
        let time_delta = match start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        self.ssuc_stats.ssus_append_time += time_delta;
        self.ssuc_stats.ssus_appended_bytes += bytes_appended;
    }

    pub fn update_truncate_stats(&mut self, start_time: &SystemTime, bytes_removed: usize) {
        let time_delta = match start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        self.ssuc_stats.ssus_truncate_time += time_delta;
        self.ssuc_stats.ssus_truncated_bytes += bytes_removed;
    }

    pub fn eprint_summary_stats(&self) {
        if !self.ssuc_options.quiet {
            self.ssuc_stats.eprint_summary_stats();
        }
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
        self.ssuc_source_version = Some(source_version);
        self.ssuc_destination_version = Some(destination_version);
    }

    pub fn get_source_version(&self) -> anyhow::Result<SendVersion> {
        match self.ssuc_source_version {
            Some(version) => Ok(version),
            None => anyhow::bail!("Source version not set"),
        }
    }

    pub fn get_destination_version(&self) -> anyhow::Result<SendVersion> {
        match self.ssuc_destination_version {
            Some(version) => Ok(version),
            None => anyhow::bail!("Destination version not set"),
        }
    }

    // Note: For lifetime reasons, we cannot use Self here and we need to use
    // the underlying type instead
    pub fn return_child(&mut self, child: &mut SendStreamUpgradeContext) {
        self.ssuc_stats = child.ssuc_stats;
        // Returning this to its parent
        // Ensure that it hasn't been freed already
        if !child.ssuc_associated_with_parent {
            panic!("Returning freed context!");
        }
        child.ssuc_associated_with_parent = false;
        trace!(
            self.ssuc_logger,
            "Ctxt associated with parent is {}",
            child.ssuc_associated_with_parent
        );
    }
}

impl<'a> Drop for SendStreamUpgradeContext<'a> {
    fn drop(&mut self) {
        // Ensure that any parents have been disassociated with their
        // children
        if self.ssuc_associated_with_parent {
            crit!(
                self.ssuc_logger,
                "Dangling context found! bt for context is {:?}",
                self.ssuc_backtrace
            );
        }
    }
}
