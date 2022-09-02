/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

pub use btrfs_send_stream_upgrade_lib::mp::threads::coordinator::*;

#[test]
fn test_get_thread_counts() -> anyhow::Result<()> {
    for given_thread_count in 0..256 {
        for cpu_count in 1..256 {
            let (num_command_constructors, num_compressors) =
                Coordinator::get_thread_counts(given_thread_count, cpu_count);
            let cap = if given_thread_count == 0 {
                cpu_count / 2
            } else {
                std::cmp::min(given_thread_count, cpu_count / 2)
            };
            if cap <= NUM_THREAD_TYPES {
                anyhow::ensure!(
                    num_command_constructors == 1,
                    "Expected one command constructor saw {} given {} cpus {}",
                    num_command_constructors,
                    given_thread_count,
                    cpu_count
                );
                anyhow::ensure!(
                    num_compressors == 1,
                    "Expected one compressor saw {} given {} cpus {}",
                    num_compressors,
                    given_thread_count,
                    cpu_count
                );
            } else {
                anyhow::ensure!(
                    NON_MP_THREAD_TYPES + num_command_constructors + num_compressors == cap,
                    "Mismatched thread counts command constructors {} compressors {} given {} cpus {}",
                    num_command_constructors,
                    num_compressors,
                    given_thread_count,
                    cpu_count
                );
                anyhow::ensure!(
                    num_command_constructors <= MAX_COMMAND_CONSTRUCTION_THREADS,
                    "Too many command construction threads {} cap {} given {} cpus {}",
                    num_command_constructors,
                    MAX_COMMAND_CONSTRUCTION_THREADS,
                    given_thread_count,
                    cpu_count
                );
                anyhow::ensure!(
                    num_compressors / num_command_constructors
                        >= (COMPRESSOR_THREAD_COUNT_RATIO
                            / COMMAND_CONSTRUCTION_THREAD_COUNT_RATIO),
                    "Mismatch between command construction count {} compressor count {} given {} cpus {}",
                    num_command_constructors,
                    num_compressors,
                    given_thread_count,
                    cpu_count
                );
            }
        }
    }
    Ok(())
}
