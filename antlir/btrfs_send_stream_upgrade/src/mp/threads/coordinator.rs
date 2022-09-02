/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::thread;
use std::time;

use slog::crit;
use slog::info;

use crate::mp::threads::batcher_worker::BatcherWorker;
use crate::mp::threads::command_construction_worker::CommandConstructionWorker;
use crate::mp::threads::compression_worker::CompressionWorker;
use crate::mp::threads::prefetch_worker::PrefetchWorker;
use crate::mp::threads::read_worker::ReadWorker;
use crate::mp::threads::worker_thread::WorkerThread;
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

// Check for completion at least once every 100 ms
const MIN_THREAD_COMPLETION_CHECK: time::Duration = time::Duration::from_millis(100);
// Check for completion at most once every 1 s
const MAX_THREAD_COMPLETION_CHECK: time::Duration = time::Duration::from_secs(1);

// Check for completion every 1ms during teardown
const THREAD_TEARDOWN_CHECK: time::Duration = time::Duration::from_millis(1);

pub struct Coordinator<'a> {
    /// The base context
    c_context: Option<SendStreamUpgradeContext<'a>>,
}

impl<'a> Coordinator<'a> {
    pub fn new(context: Option<SendStreamUpgradeContext<'a>>) -> anyhow::Result<Self> {
        Ok(Self { c_context: context })
    }
    pub fn run(&mut self) -> anyhow::Result<()> {
        let num_command_constructors: usize;
        let num_compressors: usize;

        match self.c_context {
            Some(ref mut context) => {
                // Be sure to flush the context before starting any IO on it
                context.flush()?;
                // Set up the sync container for mp accesses
                context.setup_sync_container()?;
                // Populate the thread count from the context
                (num_command_constructors, num_compressors) =
                    Self::get_thread_counts(context.ssuc_options.thread_count, num_cpus::get());
                info!(
                    context.ssuc_logger,
                    "Given {} threads {} CPUs derived {} constructors {} compressors",
                    context.ssuc_options.thread_count,
                    num_cpus::get(),
                    num_command_constructors,
                    num_compressors
                );
            }
            None => anyhow::bail!("None context in coordinator"),
        }
        let context = match self.c_context {
            Some(ref mut context) => context,
            None => anyhow::bail!("None context in coordinator"),
        };
        let mut active_workers: Vec<WorkerThread> = Vec::new();
        // Load up the single threaded workers first (except for the writer;
        // that goes at the very end)
        active_workers.push(WorkerThread::new::<PrefetchWorker>(
            "Prefetch worker".to_string(),
            context,
        )?);
        active_workers.push(WorkerThread::new::<ReadWorker>(
            "Read worker".to_string(),
            context,
        )?);
        active_workers.push(WorkerThread::new::<BatcherWorker>(
            "Batcher worker".to_string(),
            context,
        )?);

        // Load up all of the command construction threads
        for i in 0..num_command_constructors {
            let thread_name = format!("{} {}", "Command construction worker", i);
            active_workers.push(WorkerThread::new::<CommandConstructionWorker>(
                thread_name,
                context,
            )?);
        }

        // Load up all of the compression threads
        for i in 0..num_compressors {
            let thread_name = format!("{} {}", "Compression worker", i);
            active_workers.push(WorkerThread::new::<CompressionWorker>(
                thread_name,
                context,
            )?);
        }

        // Finally, push on the write worker
        active_workers.push(WorkerThread::new::<WriteWorker>(
            "Write worker".to_string(),
            context,
        )?);

        let mut completion_check_duration = MIN_THREAD_COMPLETION_CHECK;
        let mut all_done = false;
        let mut crashed = false;
        while !all_done {
            // Sleep
            thread::sleep(completion_check_duration);
            // Increment the duration
            completion_check_duration = std::cmp::min(
                completion_check_duration + MIN_THREAD_COMPLETION_CHECK,
                MAX_THREAD_COMPLETION_CHECK,
            );

            // Loop through all of the threads
            // Note that if we use a for loop with a range, it will be
            // statically calculated.
            let mut i = 0;
            while i < active_workers.len() {
                // Check to see if it is done
                match active_workers.get_mut(i) {
                    Some(ref mut worker) => {
                        match worker.get_status() {
                            // If the worker is still running, skip it for now
                            Ok(true) => {
                                i += 1;
                                continue;
                            }
                            // Is the worker done?
                            Ok(false) => {
                                // If this is the last worker, then we're truly
                                // done
                                if i == active_workers.len() - 1 {
                                    all_done = true;
                                }
                            }
                            Err(error) => {
                                // Log the error as a critical failure
                                crit!(
                                    context.ssuc_logger,
                                    "Critical fail in check thread status {}",
                                    error
                                );
                                all_done = true;
                                crashed = true;
                            }
                        }
                    }
                    None => anyhow::bail!("Failed to get last element for index {}", i,),
                };
                // Remove this element since we cannot check its status again
                active_workers.remove(i);
                i += 1;
            }
        }

        // Shut everything down
        match context.ssuc_sync_container {
            Some(ref sync_container) => sync_container.halt_all(crashed)?,
            None => anyhow::bail!("Halting None context in coordinator"),
        }

        // Wait for all threads to shut down
        while !active_workers.is_empty() {
            // Sleep
            thread::sleep(THREAD_TEARDOWN_CHECK);

            // Loop through all of the threads
            let mut i = 0;
            while i < active_workers.len() {
                // Check to see if it is done
                match active_workers.get_mut(i) {
                    Some(ref mut worker) => {
                        match worker.get_status() {
                            // If the worker is still running, skip it for now
                            Ok(true) => {
                                i += 1;
                                continue;
                            }
                            // Is the worker done?
                            Ok(false) => (),
                            Err(error) => {
                                // Log the error as a critical failure
                                crit!(
                                    context.ssuc_logger,
                                    "Critical fail in check thread status {}",
                                    error
                                );
                                crashed = true;
                            }
                        }
                    }
                    None => anyhow::bail!("Failed to get last element"),
                };
                // Remove this element since we cannot check its status again
                active_workers.remove(i);
                i += 1;
            }
        }

        if crashed {
            anyhow::bail!("Encountered critical error in coordinator")
        } else {
            Ok(())
        }
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
