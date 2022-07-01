/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Display;
use std::time::Duration;
use std::time::SystemTime;

#[derive(Clone, Copy)]
pub struct SendStreamUpgradeStats {
    /// The start time
    ssus_start_time: SystemTime,
    /// The total amount of time reading data from buffers
    pub ssus_buffer_read_time: Duration,
    /// The total amount of time reading data from storage
    pub ssus_storage_read_time: Duration,
    /// The total number of bytes read
    pub ssus_bytes_read: usize,
    /// The total number of reads issued
    pub ssus_reads_issued: usize,
    /// The total amount of time writing data from buffers
    pub ssus_buffer_write_time: Duration,
    /// The total amount of time writing data from storage
    pub ssus_storage_write_time: Duration,
    /// The total amount of time compressing data
    pub ssus_compress_time: Duration,
    /// The number of times compression passed
    pub ssus_compression_passed: usize,
    /// The number of time compression failed
    pub ssus_compression_failed: usize,
    /// The total number of uncompressed bytes that were written
    pub ssus_compressed_bytes_written: usize,
    /// The total number of compressed bytes that were written
    pub ssus_uncompressed_bytes_written: usize,
    /// The total number of bytes that were compressed and written
    /// or not compressed and written
    pub ssus_logical_bytes_written: usize,
    /// The total number of uncompressed writes issued
    pub ssus_compressed_writes_issued: usize,
    /// The total number of compressed writes issued
    pub ssus_uncompressed_writes_issued: usize,
    /// The total number of bytes copied
    pub ssus_bytes_copied: usize,
    /// The total amount of time crcing data
    pub ssus_crc32c_time: Duration,
    /// The total number of bytes crced
    pub ssus_crc32c_bytes_processed: usize,
    /// The total number of commands read
    pub ssus_commands_read: usize,
    /// The total number of command written
    pub ssus_commands_written: usize,
    /// The total amount of time appending data
    pub ssus_append_time: Duration,
    /// The total number of bytes appended
    pub ssus_appended_bytes: usize,
    /// The total amount of time truncating data
    pub ssus_truncate_time: Duration,
    /// The total number of bytes truncated
    pub ssus_truncated_bytes: usize,
}

impl SendStreamUpgradeStats {
    pub fn new() -> SendStreamUpgradeStats {
        SendStreamUpgradeStats {
            ssus_start_time: SystemTime::now(),
            ssus_buffer_read_time: Duration::new(0, 0),
            ssus_storage_read_time: Duration::new(0, 0),
            ssus_bytes_read: 0,
            ssus_reads_issued: 0,
            ssus_buffer_write_time: Duration::new(0, 0),
            ssus_storage_write_time: Duration::new(0, 0),
            ssus_compress_time: Duration::new(0, 0),
            ssus_compression_passed: 0,
            ssus_compression_failed: 0,
            ssus_compressed_bytes_written: 0,
            ssus_uncompressed_bytes_written: 0,
            ssus_logical_bytes_written: 0,
            ssus_compressed_writes_issued: 0,
            ssus_uncompressed_writes_issued: 0,
            ssus_bytes_copied: 0,
            ssus_crc32c_time: Duration::new(0, 0),
            ssus_crc32c_bytes_processed: 0,
            ssus_commands_read: 0,
            ssus_commands_written: 0,
            ssus_append_time: Duration::new(0, 0),
            ssus_appended_bytes: 0,
            ssus_truncate_time: Duration::new(0, 0),
            ssus_truncated_bytes: 0,
        }
    }

    fn eprint_one_line(string: &str, numerator: Duration, denominator: Duration) {
        eprintln!(
            "{}\t: ({:.4}%) {}usecs",
            string,
            (100.0 * numerator.as_micros() as f64) / denominator.as_micros() as f64,
            numerator.as_micros()
        );
    }

    pub fn eprint_summary_stats(&self) {
        eprintln!("Overall summary: {}", self);
        let total_time = match self.ssus_start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        let other_time = total_time
            .saturating_sub(self.ssus_buffer_read_time)
            .saturating_sub(self.ssus_storage_read_time)
            .saturating_sub(self.ssus_buffer_write_time)
            .saturating_sub(self.ssus_storage_write_time)
            .saturating_sub(self.ssus_compress_time)
            .saturating_sub(self.ssus_crc32c_time)
            .saturating_sub(self.ssus_append_time)
            .saturating_sub(self.ssus_truncate_time);
        Self::eprint_one_line("Total Time\t", total_time, total_time);
        Self::eprint_one_line("Buffer Read\t", self.ssus_buffer_read_time, total_time);
        Self::eprint_one_line("Storage Read", self.ssus_storage_read_time, total_time);
        Self::eprint_one_line("Buffer Write", self.ssus_buffer_write_time, total_time);
        Self::eprint_one_line("Storage Write", self.ssus_storage_write_time, total_time);
        Self::eprint_one_line("Compression\t", self.ssus_compress_time, total_time);
        Self::eprint_one_line("CRC32C Sums\t", self.ssus_crc32c_time, total_time);
        Self::eprint_one_line("Append Time\t", self.ssus_append_time, total_time);
        Self::eprint_one_line("Truncate Time", self.ssus_truncate_time, total_time);
        Self::eprint_one_line("Other Time\t", other_time, total_time);
    }
}

impl Display for SendStreamUpgradeStats {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let total_time = match self.ssus_start_time.elapsed() {
            Ok(duration) => duration,
            Err(_) => Duration::new(0, 0),
        };
        write!(
            f,
            "<Stats Time={:?} <Read BufferTime={:?} StorageTime={:?} Bytes={} IOs={} Commands={}/><Write BufferTime={:?} StorageTime={:?} <Compressed Time={:?} Bytes={} IOs={} Succeed={} Failed={}/><UnCompressed Bytes={} IOs={}/> LogicalBytes={} Commands={}/><Copied Bytes={}/><CRC32C Time={:?} Bytes={}/><Batching <Appended Time={:?} Bytes={}/><Truncated Time={:?} Bytes={}/>/>/>",
            total_time,
            self.ssus_buffer_read_time,
            self.ssus_storage_read_time,
            self.ssus_bytes_read,
            self.ssus_reads_issued,
            self.ssus_commands_read,
            self.ssus_buffer_write_time,
            self.ssus_storage_write_time,
            self.ssus_compress_time,
            self.ssus_compressed_bytes_written,
            self.ssus_compressed_writes_issued,
            self.ssus_compression_passed,
            self.ssus_compression_failed,
            self.ssus_uncompressed_bytes_written,
            self.ssus_uncompressed_writes_issued,
            self.ssus_logical_bytes_written,
            self.ssus_commands_written,
            self.ssus_bytes_copied,
            self.ssus_crc32c_time,
            self.ssus_crc32c_bytes_processed,
            self.ssus_append_time,
            self.ssus_appended_bytes,
            self.ssus_truncate_time,
            self.ssus_truncated_bytes
        )
    }
}
