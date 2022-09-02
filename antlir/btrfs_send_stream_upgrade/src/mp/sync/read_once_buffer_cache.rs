/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::sync::Condvar;
use std::sync::Mutex;
use std::sync::RwLock;

use crate::mp::sync::blocking_sync_primitive::BlockingSyncPrimitive;
use crate::mp::sync::blocking_sync_primitive::PrimitiveState;
use crate::mp::sync::read_buffer::ReadBuffer;
use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

struct ReadOnceBufferCacheInternal {
    /// The buffer cache is implemented as a hashmap of buffers
    robci_buffers: HashMap<usize, ReadBuffer>,
}

struct ReadOnceBufferCacheMetadata {
    /// Represents the bytes remaining to be fetched in each buffer
    robcm_bytes_remaining: HashMap<usize, usize>,
    /// Represents the next key for the prefetcher to fetch
    /// This is set to -1 once we hit EOF
    robcm_next_key_to_fetch: usize,
    /// The state of the cache
    robcm_state: PrimitiveState,
}

///
/// A Read-Once Buffer Cache
///
/// This is a data structure that allows somewhat random access to a large
/// sequential window of a file when it is guaranteed that each byte in the
/// file will be read exactly once.
///
/// Multiple "Readers" and one "Prefetcher" synchronize on this data structure
/// as follows:
///
///            read_exact    -------------    puts read
/// Reader  <- API to     <- | Read-Once | <- buffers via    <- Prefetcher
/// Threads <- fill       <- | Buffer    | <- prefetcher_loop   Thread
///            vectors       | Cache     |    API
///                          _____________
///
/// Here is the expected workflow for each type of thread:
/// * Prefetcher threads will call the prefetcher_loop API to read fixed-size
///   buffers into the cache. They will block if too many buffers have been
///   placed in the cache. Once readers have "consumed" sufficient buffers, they
///   will start filling the cache again. They will continue until they end up
///   hitting the EOF.
/// * Reader threads will call the read_exact API to copy data into byte
///   vectors. If the backing buffer has not yet been fetched, this API will
///   block until the prefetcher is able to read the required data.
///   Data copies are performed under a reader-writer lock allowing for
///   concurrent copies from a given data buffer. After a copy has concluded,
///   the number of bytes remaining to be read is decremented. Once this count
///   hits zero, the buffer is removed from the cache.
///
/// As an example, let's say we have a file with a 10B buffer size and a maximum
/// capacity of three buffers. Let us assume that the cache is in the following
/// state:
///
///  0         9    10      19    20      29
/// |X_________X|  |XXXXXX_XXX|  |__________|
///                                            ^
///                                            |
///                                            Prefetcher waiting
///
/// Let us assume that all bytes marked with "X" have been copied and bytes
/// marked with "_" have yet to be copied.
///
/// At this point, the prefetcher will be waiting for buffers to be reclaimed.
///
/// If a reader were to try to read bytes 32 to 34, it would block waiting for
/// the prefetcher leaving the cache in the following state:
///
///  0         9    10      19    20      29
/// |X_________X|  |XXXXXX_XXX|  |__________|
///                                            ^
///                                            |
///                                            Prefetcher waiting
///                                            Reader waiting @ 32 to 34
///
/// If another reader were to now read only byte 16, we would have the following
/// state in the cache:
///
///  0         9    10      19    20      29
/// |X_________X|  |XXXXXXXXXX|  |__________|
///                                            ^
///                                            |
///                                            Prefetcher waiting
///                                            Reader waiting @ 32 to 34
///
/// This would result in the active reader removing the second buffer from the
/// cache and unblocking the prefetcher to load up another buffer:
///
///  0         9    20      29
/// |X_________X|  |__________|
///                           ^
///                           |
///                           Reader waiting @ 32 to 34
///
/// Once the prefetcher reads the new buffer, it will end up unblocking the
/// remaining reader leaving the cache in the following state:
///
///  0         9    20      29    30      39
/// |X_________X|  |__________|  |__XXX_____|
///
///
/// Note that if the maximum capacity of the cache is too small, then the cache
/// can end up deadlocking. Therefore, it is important to ensure that the
/// maximum capacity of the cache is set to a value that is sufficently larger
/// than the maximum size of a copy request.
///
pub struct ReadOnceBufferCache {
    /// A reader writer lock to protect the buffers
    robc_cache: RwLock<ReadOnceBufferCacheInternal>,
    /// A mutex to protect the internal queues
    robc_metadata: Mutex<ReadOnceBufferCacheMetadata>,
    /// A condvar to have readers wait
    robc_reader_wait_cv: Condvar,
    /// A condvar to have prefetchers wait
    robc_prefetcher_wait_cv: Condvar,
    /// The size of each buffer
    robc_buffer_size: usize,
    /// The number of bytes read before the cache was constructed
    robc_cache_start_address: usize,
    /// The maximum number of buffers to cache
    robc_maximum_buffers_to_cache: usize,
}

impl ReadOnceBufferCache {
    pub fn new(
        buffer_size: usize,
        cache_start_address: usize,
        max_buffers_to_cache: usize,
    ) -> anyhow::Result<Self> {
        Ok(Self {
            robc_cache: RwLock::new(ReadOnceBufferCacheInternal {
                robci_buffers: HashMap::new(),
            }),
            robc_metadata: Mutex::new(ReadOnceBufferCacheMetadata {
                robcm_bytes_remaining: HashMap::new(),
                robcm_next_key_to_fetch: cache_start_address / buffer_size,
                robcm_state: PrimitiveState::Running,
            }),
            robc_reader_wait_cv: Condvar::new(),
            robc_prefetcher_wait_cv: Condvar::new(),
            robc_buffer_size: buffer_size,
            robc_cache_start_address: cache_start_address,
            robc_maximum_buffers_to_cache: max_buffers_to_cache,
        })
    }
    fn read_buffer_from_cache(
        &self,
        key: usize,
        offset: usize,
        slice: &mut [u8],
        slice_offset: usize,
    ) -> anyhow::Result<usize> {
        // First lock to get the metadata
        let metadata = match self.robc_metadata.lock() {
            Ok(metadata) => metadata,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => {
                anyhow::bail!("Failed to acquire lock on read buffer with error {}", error)
            }
        };
        // Wait until we're certain that our buffer has been fetched
        let metadata = match self.robc_reader_wait_cv.wait_while(metadata, |metadata| {
            // Bail if we're not running
            if (*metadata).robcm_state != PrimitiveState::Running {
                return false;
            }
            // Wait while bytes remaining is not a non-zero value
            match (*metadata).robcm_bytes_remaining.get(&key) {
                None => true,
                Some(0) => true,
                _ => false,
            }
        }) {
            Ok(metadata) => metadata,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to wait with error {}", error),
        };
        match (*metadata).robcm_state {
            PrimitiveState::Running => (),
            PrimitiveState::Aborted => {
                anyhow::bail!("Reading buffer failed because of early abort");
            }
            PrimitiveState::Done => (),
        }
        // We should have something to read now
        // Drop the lock on the metadata so that we can grab an RW lock
        Mutex::unlock(metadata);

        let cache = match self.robc_cache.read() {
            Ok(cache) => cache,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => {
                anyhow::bail!("Failed to acquire lock on read buffer with error {}", error)
            }
        };
        let buffer = match (*cache).robci_buffers.get(&key) {
            Some(buffer) => buffer,
            // This should never happen
            None => anyhow::bail!("Failed to get buffer for read"),
        };

        // Read from the buffer
        buffer.read(offset, slice, slice_offset)
    }
    fn put_buffer(&self, buffer: ReadBuffer) -> anyhow::Result<()> {
        let key = buffer.get_key();
        let size = buffer.get_size();
        // Put the page in place
        {
            // Grab a write lock
            let mut cache = match self.robc_cache.write() {
                Ok(cache) => cache,
                // This should only happen if another thread panicked because of
                // a recursive lock
                Err(error) => {
                    anyhow::bail!("Failed to acquire lock on put buffer with error {}", error)
                }
            };
            // Put the page in place
            if size != 0 {
                let old_entry = (*cache).robci_buffers.insert(key, buffer);

                match old_entry {
                    Some(entry) => anyhow::bail!(
                        "Inserting duplicate entry in read buffer cache {:?} at {} {}",
                        entry,
                        key,
                        size
                    ),
                    None => (),
                }
            }
            // This should drop the write lock
            // (There isn't an explicit unlock in the API today)
            // Note that this means that this RW lock is dropped before we grab
            // the regular lock below
        }

        // Fix up the bytes to read and other metadata
        // First lock to get the metadata
        let mut metadata = match self.robc_metadata.lock() {
            Ok(metadata) => metadata,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => {
                anyhow::bail!("Failed to acquire lock on put buffer with error {}", error)
            }
        };
        match (*metadata).robcm_state {
            PrimitiveState::Running => (),
            PrimitiveState::Aborted => {
                anyhow::bail!("Putting buffer failed because of early abort");
            }
            PrimitiveState::Done => {
                // We should never be done while getting a buffer...
                anyhow::bail!("Putting buffer while done");
            }
        }
        // Update bytes remaining
        if size == 0 {
            // We must have hit an EOF
            (*metadata).robcm_bytes_remaining.remove(&key);
            // Mark the state as done
            (*metadata).robcm_state = PrimitiveState::Done;
        } else {
            (*metadata).robcm_bytes_remaining.insert(key, size);
        }
        self.robc_reader_wait_cv.notify_all();

        Ok(())
    }
    fn get_buffer_to_fill(&self) -> anyhow::Result<Option<ReadBuffer>> {
        // First lock to get the metadata
        let metadata = match self.robc_metadata.lock() {
            Ok(metadata) => metadata,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!(
                "Failed to acquire lock on get buffer to fill with error {}",
                error
            ),
        };
        // Wait if we've got too many elements sitting around
        let mut metadata = match self
            .robc_prefetcher_wait_cv
            .wait_while(metadata, |metadata| {
                (*metadata).robcm_bytes_remaining.len() == self.robc_maximum_buffers_to_cache
                    && (*metadata).robcm_state == PrimitiveState::Running
            }) {
            Ok(metadata) => metadata,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to wait with error {}", error),
        };
        match (*metadata).robcm_state {
            PrimitiveState::Running => (),
            PrimitiveState::Aborted => {
                anyhow::bail!("Putting buffer failed because of early abort");
            }
            PrimitiveState::Done => {
                // We might be finished
                return Ok(None);
            }
        }
        // Get the next key to fetch
        let key = (*metadata).robcm_next_key_to_fetch;
        (*metadata).robcm_next_key_to_fetch += 1;
        // Drop the lock
        Mutex::unlock(metadata);

        // Populate the buffer and return it
        // Do this after the lock is dropped
        let offset = if key == self.robc_cache_start_address / self.robc_buffer_size {
            self.robc_cache_start_address % self.robc_buffer_size
        } else {
            0
        };
        // Be sure to shrink the buffer if the offset is non-zero
        let read_buffer = ReadBuffer::new(key, offset, self.robc_buffer_size - offset)?;

        Ok(Some(read_buffer))
    }
    fn update_bytes_read(&self, key: usize, bytes_read: usize) -> anyhow::Result<()> {
        // First lock to get the metadata
        let mut metadata = match self.robc_metadata.lock() {
            Ok(metadata) => metadata,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!(
                "Failed to acquire lock on update bytes with error {}",
                error
            ),
        };
        match (*metadata).robcm_state {
            PrimitiveState::Running => (),
            PrimitiveState::Aborted => {
                anyhow::bail!("Putting buffer failed because of early abort");
            }
            PrimitiveState::Done => (),
        }
        let new_value = match (*metadata).robcm_bytes_remaining.get(&key) {
            Some(current_bytes) => {
                // Ensure that we don't end up underflowing the bytes read
                // count. This is a memory leak that can occur when the users
                // of the read-once buffer cache aren't reading once.
                anyhow::ensure!(
                    *current_bytes >= bytes_read,
                    "Underflowing bytes used from {} by removing {}",
                    *current_bytes,
                    bytes_read,
                );
                *current_bytes - bytes_read
            }
            // This should never happen
            None => anyhow::bail!("Failed to get bytes to read"),
        };
        if new_value == 0 {
            // Garbage collect the old bytes remaining
            (*metadata).robcm_bytes_remaining.remove(&key);
            // Wait any producers
            self.robc_prefetcher_wait_cv.notify_all();
        } else {
            // Update the new value
            (*metadata).robcm_bytes_remaining.insert(key, new_value);
        }
        // Drop the lock on the metadata so that we can grab an RW lock if
        // necessary
        Mutex::unlock(metadata);

        if new_value == 0 {
            // Grab a write lock to garbage collect the page
            let mut cache = match self.robc_cache.write() {
                Ok(cache) => cache,
                // This should only happen if another thread panicked because of
                // a recursive lock
                Err(error) => anyhow::bail!(
                    "Failed to acquire lock on update bytes read with error {}",
                    error
                ),
            };
            // Remove the page
            (*cache).robci_buffers.remove(&key);
            // The page should be removed once we exit this scope
        }

        Ok(())
    }
    pub fn read_exact(&self, slice: &mut [u8], start_offset: usize) -> anyhow::Result<()> {
        let mut slice_offset = 0;
        while slice_offset < slice.len() {
            let key = (start_offset + slice_offset) / self.robc_buffer_size;
            let offset = (start_offset + slice_offset) % self.robc_buffer_size;
            let bytes_read: usize;
            {
                bytes_read = self.read_buffer_from_cache(key, offset, slice, slice_offset)?;
                anyhow::ensure!(bytes_read != 0, "Failed to write any bytes to buffer");
            }
            // Note: This will free up the buffer if necessary
            self.update_bytes_read(key, bytes_read)?;
            slice_offset += bytes_read;
        }
        Ok(())
    }
    // A note on the prefetch threads:
    //
    // The expectation is that the send stream upgrade tool will most often
    // run against an input pipe.
    // That presents a few challenges vis-a-vis multithreaded reads on a given
    // data source.
    // For one thing, if two write syscalls are outstandin, then I can't think
    // of a good way to ensure which one completes first, and more importantly
    // which one completed at which offset.
    //
    // As a example, if I am to dispatch two 1MiB reads from thread A and
    // thread B:
    //
    // * Thread A -> Reads 1MiB first (possibly from 0 to 1MiB?)
    // * Thread B -> Reads 1MiB next (possibly from 1MiB to 2MiB?)
    //
    // If both IOs are outstanding at the same time against the same file
    // descriptor, how do we tell if thread A has the 0 to 1MiB region
    // or the 1MiB or 2MiB region in hand either before or after the
    // read request has completed?
    //
    // Morever, without holding some kind of critical section around the write
    // dispatch, how can we even ensure that the read for thread A is dispatched
    // before the read for thread B?
    //
    // At first glance, "pread" and its ilk might seem like potential solutions
    // to this conundrum. Let's have the write request specify the ranges:
    //
    // * Thread A -> pRead from 0 to 1MiB
    // * Thread B -> pRead from 1MiB to 2MiB
    //
    // However, what happens if Thread B's read is scheduled before Thread A's
    // read? _If_ a seek were to be allowed on the file in question, wouldn't
    // the 0 to 1MiB region be pushed out of the FIFO first before the read
    // is serviced?
    //
    // One resolution for this problem would be to have Thread B block to wait
    // for Thread A to first retrive the data in question. But what would happen
    // if Thread A were never to arrive?
    //
    // Finally, as per the Linux source code, it would appear that pread is not
    // supported on a PIPE (probably to avoid confusing semantics like this):
    //
    // In fs/read_write.c @ ksys_pread64:
    //
    //    if (f.file) {
    //            ret = -ESPIPE;
    //            if (f.file->f_mode & FMODE_PREAD)
    //                    ret = vfs_read(f.file, buf, count, &pos);
    //            fdput(f);
    //    }
    //
    // It seems reasonable to assumed that FMODE_PREAD is not expected to be set
    // for pipes, meaning that pread isn't supported here.
    //
    // In lieu of all of these complications, only one prefetcher thread is
    // currently supported. Using an aio_read interface for this thread in the
    // future might be something worth investigating in the future.
    pub fn prefetcher_loop(&self, context: &mut SendStreamUpgradeContext) -> anyhow::Result<()> {
        loop {
            // Try to get a buffer to fill
            let mut buffer = match self.get_buffer_to_fill()? {
                // Did we hit the end?
                None => return Ok(()),
                Some(buffer) => buffer,
            };

            buffer.fill_read_buffer(context)?;
            // Validate the start and the end for the buffer
            let first_cache_key = self.robc_cache_start_address / self.robc_buffer_size;
            let start_address = buffer.get_key() * self.robc_buffer_size + buffer.get_offset();
            let start_offset = start_address % self.robc_buffer_size;
            let end_address = start_address + buffer.get_size();
            let end_offset = end_address % self.robc_buffer_size;
            let end_buffer_key = end_offset / self.robc_buffer_size;
            // Either we have a very tiny buffer, or the start or the end of the
            // buffer has to be correctly aligned
            // Exempt empty buffers at EOL
            anyhow::ensure!(
                first_cache_key == end_buffer_key
                    || start_offset == 0
                    || end_offset == 0
                    || buffer.get_size() == 0,
                "Found a missized buffer with start {} end {} cache start {} buffer size {}",
                start_address,
                end_address,
                self.robc_cache_start_address,
                self.robc_buffer_size
            );
            self.put_buffer(buffer)?;
        }
    }
}

impl BlockingSyncPrimitive for ReadOnceBufferCache {
    fn halt(&self, unplanned: bool) -> anyhow::Result<()> {
        // First lock to get the metadata
        let mut metadata = match self.robc_metadata.lock() {
            Ok(metadata) => metadata,
            // This should only happen if another thread panicked because of
            // a recursive lock
            Err(error) => anyhow::bail!("Failed to acquire lock on halt with error {}", error),
        };
        // Disallow transitions from Aborted to Done
        // We can go from Done to Aborted in the case of a later failure
        if (*metadata).robcm_state == PrimitiveState::Aborted && !unplanned {
            anyhow::bail!("Transitioning cache from Done to Aborted");
        }
        // Update the state
        (*metadata).robcm_state = if unplanned {
            PrimitiveState::Aborted
        } else {
            PrimitiveState::Done
        };
        // Wake everyone
        self.robc_reader_wait_cv.notify_all();
        self.robc_prefetcher_wait_cv.notify_all();

        Ok(())
    }
}
