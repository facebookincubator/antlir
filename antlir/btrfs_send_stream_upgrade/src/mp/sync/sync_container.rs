/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::sync::Arc;
use std::sync::Mutex;

use crate::mp::send_elements::command_batch_info::CommandBatchInfo;
use crate::mp::send_elements::command_info::CommandInfo;
use crate::mp::sync::blocking_queue::BlockingQueue;
use crate::mp::sync::ordered_element_queue::OrderedElementQueue;
use crate::mp::sync::read_once_buffer_cache::ReadOnceBufferCache;
use crate::mp::sync::unordered_element_queue::UnorderedElementQueue;
use crate::send_elements::send_header::SendHeader;
use crate::upgrade::send_stream_upgrade_options::SendStreamUpgradeOptions;
use crate::upgrade::send_stream_upgrade_stats::SendStreamUpgradeStats;

pub struct SyncContainer {
    /// A buffer cache for reads for the reader thread and the command
    /// construction threads; serviced by the prefetcher thread
    pub sc_buffer_cache: Arc<ReadOnceBufferCache>,
    /// A queue for commands to be constructed; populated by the reader
    /// thread and serviced by the command construction threads
    pub sc_command_construction_queue: Arc<UnorderedElementQueue<CommandInfo>>,
    /// A queue for constructed and upgrade commands ready for processing;
    /// populated by the command construction threads and serviced by the
    /// batcher thread
    pub sc_batcher_queue: Arc<OrderedElementQueue<CommandBatchInfo>>,
    /// A queue for command batches that can be appended, compressed, and
    /// crced; populated by the batcher thread and serviced by the
    /// compression threads
    pub sc_compression_queue: Arc<UnorderedElementQueue<CommandBatchInfo>>,
    /// A queue for commands that are ready for persistence; populated
    /// by the compression threads (for data commands) and the command
    /// construction threads (for metadata commands) and serviced by
    /// the writer thread
    pub sc_persistence_queue: Arc<OrderedElementQueue<CommandBatchInfo>>,
    /// Shared statistics
    pub sc_stats: Arc<Mutex<SendStreamUpgradeStats>>,
}

impl SyncContainer {
    pub fn new(options: &SendStreamUpgradeOptions) -> anyhow::Result<Self> {
        Ok(Self {
            sc_buffer_cache: Arc::new(ReadOnceBufferCache::new(
                options.read_buffer_size,
                SendHeader::get_size(),
                // TODO: Pull from option
                1000,
            )?),
            sc_command_construction_queue: Arc::new(UnorderedElementQueue::<CommandInfo>::new(
                "Command construction queue",
            )?),
            sc_batcher_queue: Arc::new(OrderedElementQueue::<CommandBatchInfo>::new(
                "Batcher queue",
            )?),
            sc_compression_queue: Arc::new(UnorderedElementQueue::<CommandBatchInfo>::new(
                "Compression queue",
            )?),
            sc_persistence_queue: Arc::new(OrderedElementQueue::<CommandBatchInfo>::new(
                "Persistence queue",
            )?),
            sc_stats: Arc::new(Mutex::new(SendStreamUpgradeStats::new())),
        })
    }
    pub fn rollover_stats(&self, other_stats: &SendStreamUpgradeStats) -> anyhow::Result<()> {
        // First lock to get at the underlying stats
        let mutex = &self.sc_stats;
        let mut stats = match mutex.lock() {
            Ok(stats) => stats,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to acquire lock for stats rollover with {}", error),
        };
        *stats += *other_stats;
        Ok(())
    }
}

impl Clone for SyncContainer {
    fn clone(&self) -> Self {
        Self {
            sc_buffer_cache: self.sc_buffer_cache.clone(),
            sc_command_construction_queue: self.sc_command_construction_queue.clone(),
            sc_batcher_queue: self.sc_batcher_queue.clone(),
            sc_compression_queue: self.sc_compression_queue.clone(),
            sc_persistence_queue: self.sc_persistence_queue.clone(),
            sc_stats: self.sc_stats.clone(),
        }
    }
}
