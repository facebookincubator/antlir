/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under both the MIT license found in the
 * LICENSE-MIT file in the root directory of this source tree and the Apache
 * License, Version 2.0 found in the LICENSE-APACHE file in the root directory
 * of this source tree.
 */

//! In-memory async buffers that implement Read.

use std::cmp;
use std::io;
use std::io::Read;
use std::mem;

use bytes::{BufMut, BytesMut};
use futures::task;
use tokio_io::AsyncRead;

/// A fixed capacity in-memory buffer that implements asynchronous Read.
#[derive(Debug)]
pub struct MemBuf {
    capacity: usize,
    buf: BytesMut,
    task: ParkedTask,
    eof: bool,
}

impl MemBuf {
    /// Create an instance of [MemBuf] with the provided buffer capacity
    pub fn new(buf_size: usize) -> Self {
        MemBuf {
            capacity: buf_size,
            buf: BytesMut::with_capacity(buf_size),
            task: ParkedTask::None,
            eof: false,
        }
    }

    /// Write the provided data to the in-memory buffer
    pub fn write_buf(&mut self, data: &[u8]) -> io::Result<usize> {
        if self.eof || data.is_empty() {
            return Ok(0);
        }

        if self.capacity == self.buf.len() {
            self.task = ParkedTask::WriteTask(task::current());
            return Err(io::Error::new(io::ErrorKind::WouldBlock, "buffer full"));
        }

        let to_write = cmp::min(self.capacity - self.buf.len(), data.len());
        self.buf.put_slice(&data[..to_write]);
        if to_write > 0 {
            // Data is now available, so this stream is unblocked.
            self.unblock_read();
        }

        Ok(to_write)
    }

    /// Notify this [MemBuf] that there won't be any more data incoming to it,
    /// it will wake any async tasks waiting for content, all attempts to read
    /// the rest of the data from this instance will succeed immediately.
    pub fn mark_eof(&mut self) {
        self.eof = true;
        self.unblock_read();
    }

    fn unblock_read(&mut self) {
        self.task = match self.task.take() {
            ParkedTask::ReadTask(ref t) => {
                (*t).notify();
                ParkedTask::None
            }
            pt => pt,
        };
    }
}

impl From<Vec<u8>> for MemBuf {
    fn from(vec: Vec<u8>) -> MemBuf {
        MemBuf {
            capacity: vec.capacity(),
            buf: BytesMut::from(vec),
            task: ParkedTask::None,
            eof: false,
        }
    }
}

impl Read for MemBuf {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.buf.is_empty() {
            if self.eof {
                return Ok(0);
            }
            self.task = ParkedTask::ReadTask(task::current());
            return Err(io::Error::new(io::ErrorKind::WouldBlock, "buffer empty"));
        }

        let len = {
            let slice = self.buf.as_ref();
            let len = cmp::min(slice.len(), buf.len());
            if len == 0 {
                return Ok(0);
            }
            let slice = &slice[..len];
            let buf = &mut buf[..len];
            buf.copy_from_slice(slice);
            len
        };

        self.buf.split_to(len);
        self.task = match self.task.take() {
            ParkedTask::WriteTask(ref t) => {
                (*t).notify();
                ParkedTask::None
            }
            pt => pt,
        };
        Ok(len)
    }
}

impl AsyncRead for MemBuf {}

#[derive(Debug)]
enum ParkedTask {
    ReadTask(task::Task),
    WriteTask(task::Task),
    None,
}

impl ParkedTask {
    pub fn take(&mut self) -> Self {
        mem::replace(self, ParkedTask::None)
    }
}
