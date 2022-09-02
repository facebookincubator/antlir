/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use slog::info;

use crate::mp::threads::batcher_worker::BatcherWorker;
use crate::mp::threads::command_construction_worker::CommandConstructionWorker;
use crate::mp::threads::compression_worker::CompressionWorker;
use crate::mp::threads::prefetch_worker::PrefetchWorker;
use crate::mp::threads::read_worker::ReadWorker;
use crate::mp::threads::worker::Worker;
use crate::mp::threads::write_worker::WriteWorker;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

// Cap the number of command construction threads
pub const MAX_COMMAND_CONSTRUCTION_THREADS: usize = 8usize;
// The ratio for the thread count for command construction threads
pub const COMMAND_CONSTRUCTION_THREAD_COUNT_RATIO: usize = 1usize;
// The ratio for the thread count for compressor threads
pub const COMPRESSOR_THREAD_COUNT_RATIO: usize = 2usize;
// The denominator for the two metioned ratios
pub const THREAD_COUNT_RATIO_DENOMINATOR: usize =
    COMMAND_CONSTRUCTION_THREAD_COUNT_RATIO + COMPRESSOR_THREAD_COUNT_RATIO;
// The total number of single-threaded threads
pub const NON_MP_THREAD_TYPES: usize = 4usize;
// The number of types of threads
pub const NUM_THREAD_TYPES: usize = 6usize;

pub struct Coordinator<'a> {
    /// The base context
    c_context: Option<SendStreamUpgradeContext<'a>>,
}

impl<'a> Coordinator<'a> {
    pub fn new(context: Option<SendStreamUpgradeContext<'a>>) -> anyhow::Result<Self> {
        Ok(Self { c_context: context })
    }
    pub fn run(&mut self) -> anyhow::Result<()> {
        let _num_command_constructors: usize;
        let _num_compressors: usize;

        match self.c_context {
            Some(ref mut context) => {
                // Be sure to flush the context before starting any IO on it
                context.flush()?;
                // Set up the sync container for mp accesses
                context.setup_sync_container()?;
                // Populate the thread count from the context
                (_num_command_constructors, _num_compressors) =
                    Self::get_thread_counts(context.ssuc_options.thread_count, num_cpus::get());
                info!(
                    context.ssuc_logger,
                    "Given {} threads {} CPUs derived {} constructors {} compressors",
                    context.ssuc_options.thread_count,
                    num_cpus::get(),
                    _num_command_constructors,
                    _num_compressors
                );
            }
            None => anyhow::bail!("None context in coordinator"),
        }
        let context = match self.c_context {
            Some(ref mut context) => context,
            None => anyhow::bail!("None context in coordinator"),
        };
        {
            let _batcher = BatcherWorker::new("Batcher worker".to_string(), context)?;
            let _command_constructor =
                CommandConstructionWorker::new("Command construction worker".to_string(), context)?;
            let _compressor = CompressionWorker::new("Compression worker".to_string(), context)?;
            let _prefetcher = PrefetchWorker::new("Prefetch worker".to_string(), context)?;
            let _reader = ReadWorker::new("Read worker".to_string(), context)?;
            let _writer = WriteWorker::new("Write worker".to_string(), context)?;
        }
        Ok(())
    }
    pub fn take_context(&mut self) -> Option<SendStreamUpgradeContext<'a>> {
        self.c_context.take()
    }
    pub fn get_thread_counts(given_thread_count: usize, cpu_count: usize) -> (usize, usize) {
        // Fall back to parallelism mode if we are capped on thread count
        // This effectively defines a minimum thread count
        let half_cpu_count = cpu_count / 2;
        let max_thread_count = if given_thread_count == 0 {
            half_cpu_count
        } else {
            // Cap based on the number of physical CPUs
            std::cmp::min(given_thread_count, half_cpu_count)
        };
        // If we do not have enough threads, fall back to pipeline mode
        if max_thread_count <= NUM_THREAD_TYPES {
            return (1, 1);
        }
        // Discount the fixed threads to get the variable thread count
        let variable_thread_count = max_thread_count - NON_MP_THREAD_TYPES;
        // Derive the number of command construction threads based on the ratio
        // Cap appropriately
        let command_construction_thread_count = std::cmp::min(
            variable_thread_count * COMMAND_CONSTRUCTION_THREAD_COUNT_RATIO
                / THREAD_COUNT_RATIO_DENOMINATOR,
            MAX_COMMAND_CONSTRUCTION_THREADS,
        );
        // All remaining threads are commpressor threads
        let compressor_thread_count = variable_thread_count - command_construction_thread_count;
        (command_construction_thread_count, compressor_thread_count)
    }
}
