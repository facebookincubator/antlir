/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::backtrace::Backtrace;
use std::fs::OpenOptions;
use std::io::BufReader;
use std::io::BufWriter;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
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
use crate::upgrade::send_stream_upgrade_options::SendStreamUpgradeOptions;
use crate::upgrade::send_stream_upgrade_stats::SendStreamUpgradeStats;

pub struct SendStreamUpgradeContext<'a> {
    /// Stats related to the current upgrade context
    pub ssuc_stats: SendStreamUpgradeStats,
    /// Logger associated with the current context
    pub ssuc_logger: Logger,
    /// Options that dicatate how the stream will be upgraded
    pub ssuc_options: SendStreamUpgradeOptions,
    /// Source for the IO context
    ssuc_source: Option<BufReader<Box<dyn Read + Send + 'a>>>,
    /// Destination for the IO context
    ssuc_destination: Option<BufWriter<Box<dyn Write + Send + 'a>>>,
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
    /// Check to see if this has a parent associated with it
    ssuc_associated_with_parent: bool,
    /// Backtrace of where the context was allocated
    ssuc_backtrace: Backtrace,
    /// An mp-safe container for synchronization primitives
    pub ssuc_sync_container: Option<SyncContainer>,
}

impl<'a> SendStreamUpgradeContext<'a> {
    pub fn new(options: SendStreamUpgradeOptions) -> anyhow::Result<SendStreamUpgradeContext<'a>> {
        // Open up the input file
        let input_file: Box<dyn Read + Send + Sync> = match &options.input {
            None => Box::new(std::io::stdin()),
            Some(value) => Box::new(OpenOptions::new().read(true).open(value)?),
        };
        // Open up the output file
        let output_file: Box<dyn Write + Send + Sync> = match &options.output {
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
            ssuc_source: Some(BufReader::with_capacity(read_buffer_size, read_box as _)),
            ssuc_destination: Some(BufWriter::with_capacity(write_buffer_size, write_box as _)),
            ssuc_source_version: None,
            ssuc_destination_version: None,
            ssuc_source_offset: 0,
            ssuc_destination_offset: 0,
            ssuc_source_length: None,
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
        let read_length = read.map(|r| r.len());
        let read_box = read.map(Box::new);
        let write_box = write.map(Box::new);
        let start_time = SystemTime::now();
        let mut new_context = SendStreamUpgradeContext {
            ssuc_stats: self.ssuc_stats,
            ssuc_logger: self.ssuc_logger.clone(),
            ssuc_options: self.ssuc_options.clone(),
            ssuc_source: read_box.map(|read_box_some| BufReader::new(read_box_some as _)),
            ssuc_destination: write_box.map(|write_box_some| BufWriter::new(write_box_some as _)),
            ssuc_source_version: Some(source_version),
            ssuc_destination_version: Some(destination_version),
            ssuc_source_offset: 0,
            ssuc_destination_offset: 0,
            ssuc_source_length: read_length,
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
        sync_container: Option<SyncContainer>,
    ) -> anyhow::Result<SendStreamUpgradeContext<'a>> {
        // Open up the input file
        let source = match keep_source {
            true => {
                let input_file: Box<dyn Read + Send + Sync> = match &options.input {
                    None => Box::new(std::io::stdin()),
                    Some(value) => {
                        let mut file = OpenOptions::new().read(true).open(value)?;
                        // If we had a file input, then we need to seek forward
                        // on the underlying file
                        file.seek(SeekFrom::Start(source_offset as u64))?;
                        Box::new(file)
                    }
                };
                let read = input_file;
                let read_box = Box::new(read);
                let read_buffer_size = options.read_buffer_size;
                Some(BufReader::with_capacity(read_buffer_size, read_box as _))
            }
            false => None,
        };
        // Open up the output file
        let destination = match keep_destination {
            true => {
                let output_file: Box<dyn Write + Send + Sync> = match &options.output {
                    None => Box::new(std::io::stdout()),
                    Some(value) => {
                        // If we had a file output, then we need to seek forward
                        // on the underlying file
                        // NOTE: Unlike in the new case, don't truncate the file
                        // since we're reopening it
                        let mut file = OpenOptions::new().write(true).open(value)?;
                        file.seek(SeekFrom::Start(destination_offset as u64))?;
                        Box::new(file)
                    }
                };
                let write = output_file;
                let write_box = Box::new(write);
                let write_buffer_size = options.write_buffer_size;
                Some(BufWriter::with_capacity(write_buffer_size, write_box as _))
            }
            false => None,
        };

        Ok(SendStreamUpgradeContext {
            ssuc_stats: SendStreamUpgradeStats::new(),
            ssuc_logger: logger,
            ssuc_options: options,
            ssuc_source: source,
            ssuc_destination: destination,
            ssuc_source_version: Some(source_version),
            ssuc_destination_version: Some(destination_version),
            ssuc_source_offset: source_offset,
            ssuc_destination_offset: destination_offset,
            ssuc_source_length: None,
            ssuc_associated_with_parent: false,
            ssuc_backtrace: Backtrace::capture(),
            ssuc_sync_container: sync_container,
        })
    }

    pub fn read(&mut self, buffer: &mut [u8]) -> anyhow::Result<usize> {
        let start_time = SystemTime::now();
        let mut total_bytes_read = 0;
        match self.ssuc_source {
            Some(ref mut source) => {
                while total_bytes_read < buffer.len() {
                    // Try to read some data
                    let bytes_read = source.read(&mut buffer[total_bytes_read..])?;
                    // If we read nothing, then assume that we hit the EOF
                    // Time to exit
                    if bytes_read == 0 {
                        break;
                    }
                    total_bytes_read += bytes_read;
                }
            }
            None => {
                // Try to look into the sync container to get at the buffer
                // cache
                match self.ssuc_sync_container {
                    Some(ref mut sync_container) => {
                        match sync_container.sc_buffer_cache {
                            Some(ref buffer_cache) => {
                                // Got a buffer cache; do an exact read
                                (*buffer_cache).read_exact(buffer, self.ssuc_source_offset)?;
                            }
                            None => anyhow::bail!("Reading with None buffer cache"),
                        }
                    }
                    None => anyhow::bail!("Reading with None container"),
                }
                // If we got here, then we must have read everything
                total_bytes_read += buffer.len();
            }
        }
        let time_delta = match start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        // Increment the appropriate stats if this is not a child
        // since children are not associated with actual file io
        if !self.ssuc_associated_with_parent {
            self.ssuc_stats.ssus_reads_issued += 1;
            self.ssuc_stats.ssus_bytes_read += total_bytes_read;
            self.ssuc_stats.ssus_storage_read_time += time_delta;
        } else {
            self.ssuc_stats.ssus_buffer_read_time += time_delta;
        }
        self.ssuc_source_offset += total_bytes_read;
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
        self.ssuc_source_offset
    }

    pub fn adjust_read_offset(&mut self, increment: usize) {
        self.ssuc_source_offset += increment;
    }

    pub fn get_read_len(&self) -> anyhow::Result<usize> {
        match self.ssuc_source_length {
            Some(length) => Ok(length),
            None => anyhow::bail!("Attempted to get a length of a non-buffer source"),
        }
    }

    pub fn seek_source(&mut self, bytes_to_seek: usize) -> anyhow::Result<()> {
        // Just read the data into a temporary buffer
        let mut temp_vec = vec![0u8; bytes_to_seek];
        self.read_exact(&mut temp_vec[..])
    }

    pub fn write(&mut self, buffer: &[u8], uncompressed_bytes: usize) -> anyhow::Result<()> {
        let destination = match self.ssuc_destination {
            None => anyhow::bail!("Trying to write to a None destination"),
            Some(ref mut destination) => destination,
        };
        let start_time = SystemTime::now();
        destination.write_all(buffer)?;
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
            "Ctxt associated with parent is {}",
            child.ssuc_associated_with_parent
        );
        self.ssuc_stats.ssus_context_return_time += Self::get_time_delta(&start_time);
    }

    pub fn setup_sync_container(&mut self) -> anyhow::Result<()> {
        self.ssuc_sync_container = Some(SyncContainer::new(&self.ssuc_options)?);
        Ok(())
    }

    pub fn flush(&mut self) -> anyhow::Result<()> {
        match self.ssuc_destination {
            Some(ref mut destination) => match destination.flush() {
                Ok(()) => Ok(()),
                Err(e) => anyhow::bail!(e),
            },
            None => anyhow::bail!("Attempting to flush without a destination"),
        }
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
