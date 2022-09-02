/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::cmp::min;

use crate::upgrade::send_stream_upgrade_context::SendStreamUpgradeContext;

#[derive(Debug)]
pub struct ReadBuffer {
    /// The contents of the buffer
    rb_buffer: Vec<u8>,
    /// The key of the buffer
    rb_key: usize,
    /// The start offset of the buffer
    rb_start_offset: usize,
}

unsafe impl Send for ReadBuffer {}

impl ReadBuffer {
    pub fn new(key: usize, start_offset: usize, initial_size: usize) -> anyhow::Result<Self> {
        Ok(Self {
            rb_buffer: vec![0; initial_size],
            rb_key: key,
            rb_start_offset: start_offset,
        })
    }
    pub fn read(
        &self,
        offset: usize,
        slice: &mut [u8],
        slice_offset: usize,
    ) -> anyhow::Result<usize> {
        anyhow::ensure!(
            offset >= self.rb_start_offset,
            "Received a start offset {} that is less than minimum {}",
            offset,
            self.rb_start_offset
        );
        let buffer_offset = offset - self.rb_start_offset;
        // Stop at whatever's closer -- the end of the buffer or the end of the
        // slice
        let bytes_to_copy = min(
            self.rb_buffer.len() - buffer_offset,
            slice.len() - slice_offset,
        );
        slice[slice_offset..slice_offset + bytes_to_copy]
            .copy_from_slice(&self.rb_buffer[buffer_offset..buffer_offset + bytes_to_copy]);
        Ok(bytes_to_copy)
    }
    pub fn fill_read_buffer(
        &mut self,
        context: &mut SendStreamUpgradeContext,
    ) -> anyhow::Result<()> {
        let bytes_filled = context.read(&mut self.rb_buffer)?;
        if bytes_filled < self.rb_buffer.len() {
            self.rb_buffer.truncate(bytes_filled);
        }
        Ok(())
    }
    pub fn get_size(&self) -> usize {
        self.rb_buffer.len()
    }
    pub fn get_offset(&self) -> usize {
        self.rb_start_offset
    }
    pub fn get_key(&self) -> usize {
        self.rb_key
    }
}
