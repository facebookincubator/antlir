/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::io::Write;
use std::sync::Arc;
use std::thread;
use std::time;

pub use btrfs_send_stream_upgrade_lib::mp::sync::blocking_queue::BlockingQueue;
pub use btrfs_send_stream_upgrade_lib::mp::sync::blocking_sync_primitive::BlockingSyncPrimitive;
pub use btrfs_send_stream_upgrade_lib::mp::sync::read_once_buffer_cache::ReadOnceBufferCache;
pub use btrfs_send_stream_upgrade_lib::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;
pub use btrfs_send_stream_upgrade_lib::upgrade::send_stream_upgrade_options::SendStreamUpgradeOptions;
use rand::thread_rng;
use rand::Rng;
use structopt::StructOpt;

const FIVE_MILLISECONDS: time::Duration = time::Duration::from_millis(5);
const NUM_READERS: usize = 7;
const MAXIMUM_NUMBER: u16 = 48610;
const MAX_HALF_START_OFFSET: u16 = 50;
const BUFFER_SIZE: usize = 13;
const MAX_BUFFERS_TO_CACHE: usize = 3;
const MAX_HALF_LENGTH: u16 = 9;

fn reader(cache: &ReadOnceBufferCache, to_read: Vec<u16>, id: usize) {
    let length = to_read.len();
    if length % 2 != 0 {
        panic!("Expected an even number of elements, got {}", length);
    }
    let mut last_end = 0;
    for i in 0..length / 2 {
        let start = to_read[2 * i];
        let end = to_read[2 * i + 1];
        if start >= end {
            panic!("Got bad start {} end {}", start, end);
        }
        if i > 0 && start <= last_end {
            panic!("Got bad last_end {} start {}", last_end, start);
        }
        last_end = end;
        // Remember to multiply by two because a u16 is 2 bytes
        let mut vec = vec![0u8; 2 * (end - start) as usize];
        println!("READER {}: Trying to read from {} to {}", id, start, end);
        if let Err(error) = cache.read_exact(&mut vec[..], 2 * start as usize) {
            panic!("Reader failed during read with error {}", error);
        }
        println!("READER {}: Read from {} to {}", id, start, end);
        for j in start..end {
            let offset = (j - start) as usize;
            let first = vec[2 * offset];
            let second = vec[2 * offset + 1];
            let buffer = [first, second];
            let value = <u16>::from_le_bytes(buffer);
            if j != value {
                panic!("Reader failed on iteration {} : {} got {}", i, j, value);
            }
        }
        // Sleep a bit between items
        thread::sleep(FIVE_MILLISECONDS);
        println!("READER {}: Done iteration {}", id, i);
    }
    println!("READER {}: DONE", id);
}

fn prefetcher(cache: &ReadOnceBufferCache, mut context: SendStreamUpgradeContext) {
    if let Err(error) = cache.prefetcher_loop(&mut context) {
        panic!("Prefetcher failed with error {}", error);
    }
}

fn populate_tempfile(tempfile: &mut tempfile::NamedTempFile) -> anyhow::Result<()> {
    for i in 0u16..MAXIMUM_NUMBER {
        let bytes = i.to_le_bytes();
        tempfile.write_all(&bytes)?;
    }
    match tempfile.flush() {
        Ok(()) => Ok(()),
        Err(error) => anyhow::bail!(error),
    }
}

fn populate_read_ranges(vec: &mut [Vec<u16>]) -> anyhow::Result<u16> {
    let mut rng = thread_rng();
    let mut reader = 0usize;
    let start_offset = 2 * (rng.gen::<u16>() % MAX_HALF_START_OFFSET);
    let mut start = start_offset;
    while start < MAXIMUM_NUMBER {
        // Since u16 is two bytes long, let's ensure that all lengths
        // are multiples of two to keep both bytes together
        // Also skip zero values
        let length: u16 = 2 * (rng.gen::<u16>() % MAX_HALF_LENGTH + 1u16);
        let end = std::cmp::min(start + length, MAXIMUM_NUMBER);
        vec[reader].push(start);
        vec[reader].push(end);
        start = end;
        reader += 1;
        if reader == NUM_READERS {
            reader = 0;
        }
    }
    Ok(start_offset)
}

#[test]
fn resequence_io_read_from_file() -> anyhow::Result<()> {
    // Set up a temporary file for IO
    let mut tempfile = tempfile::NamedTempFile::new()?;
    populate_tempfile(&mut tempfile)?;

    // Set up an IO context for the file in question
    let dummy_vector = vec!["--help"];
    let mut options = SendStreamUpgradeOptions::from_iter(dummy_vector);
    options.input = Some(tempfile.path().to_path_buf());
    let mut context = SendStreamUpgradeContext::new(options)?;

    let mut vec: Vec<Vec<u16>> = vec![vec![]; NUM_READERS];
    let start_address = 2 * populate_read_ranges(&mut vec)? as usize;
    let cache = Arc::new(ReadOnceBufferCache::new(
        BUFFER_SIZE,
        start_address,
        MAX_BUFFERS_TO_CACHE,
    )?);
    // Skip the correct number of bytes in the IO context
    // This is to simulate skipping bytes for the header before
    // commands are processed
    context.seek_source(start_address)?;

    println!("Start address is {}", start_address);

    let mut readers = Vec::<thread::JoinHandle<_>>::new();

    // Start up the readers
    for i in 0..NUM_READERS {
        let thread_local_vec = vec.remove(0);
        let thread_local_cache = cache.clone();
        let reader = thread::spawn(move || reader(&thread_local_cache, thread_local_vec, i));
        readers.push(reader);
    }

    // Start up a single prefetcher
    let thread_local_cache = cache.clone();
    let prefetcher = thread::spawn(move || prefetcher(&thread_local_cache, context));

    // Check to see if any reader had errors
    for i in 0..NUM_READERS {
        let reader = match readers.pop() {
            Some(join_handle) => join_handle,
            None => anyhow::bail!("Failed to retrieve join handle for reader {}", i),
        };
        if let Err(error) = reader.join() {
            // Halt the queue to stop the test
            cache.halt(true)?;
            anyhow::bail!("Reader failed with error {:#?}", error);
        }
    }

    // Check to see if the prefetchers failed
    if let Err(error) = prefetcher.join() {
        anyhow::bail!("Prefetcher failed with error {:#?}", error);
    }

    Ok(())
}
