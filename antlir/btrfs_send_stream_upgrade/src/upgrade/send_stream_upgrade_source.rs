/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::OpenOptions;
use std::io::BufReader;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;

use crate::mp::sync::read_once_buffer_cache::ReadOnceBufferCache;
use crate::send_elements::send_version::SendVersion;

enum SendStreamSource<'a> {
    /// A BufReader source for IO from stdin or a file
    SssBufReader(BufReader<Box<dyn Read + Send>>),
    /// A slice source
    SssSliceReader(&'a [u8]),
    /// A buffer cache
    SssReadOnceBufferCache(Arc<ReadOnceBufferCache>),
    /// No source at all
    SssNoSource,
}

pub struct SendStreamUpgradeSource<'a> {
    /// The source for the send stream data
    ssus_source: SendStreamSource<'a>,
    /// An offset into the send stream data source
    ssus_offset: usize,
    /// The version of the send stream data
    ssus_version: Option<SendVersion>,
}

impl<'a> SendStreamUpgradeSource<'a> {
    pub fn new_from_file(
        input: Option<PathBuf>,
        read_buffer_size: usize,
        offset: usize,
        version: Option<SendVersion>,
    ) -> anyhow::Result<Self> {
        // Open up the input file
        let input_file: Box<dyn Read + Send + Sync> = match input {
            // Ignore the seek for stdin -- this is a pipe, and the stream
            // will be at the right spot
            None => Box::new(std::io::stdin()),
            Some(ref value) => {
                let mut file = OpenOptions::new().read(true).open(value)?;
                // If we had a file input, then we need to seek forward
                // on the underlying file
                if offset != 0 {
                    file.seek(SeekFrom::Start(offset as u64))?;
                }
                Box::new(file)
            }
        };
        Ok(Self {
            ssus_source: SendStreamSource::SssBufReader(BufReader::with_capacity(
                read_buffer_size,
                input_file,
            )),
            ssus_offset: offset,
            ssus_version: version,
        })
    }
    pub fn new_from_slice(
        slice: Option<&'a [u8]>,
        offset: usize,
        version: Option<SendVersion>,
    ) -> SendStreamUpgradeSource<'a> {
        let source = match slice {
            Some(s) => SendStreamSource::SssSliceReader(s),
            None => SendStreamSource::SssNoSource,
        };
        Self {
            ssus_source: source,
            ssus_offset: offset,
            ssus_version: version,
        }
    }
    pub fn new_from_buffer_cache(
        buffer_cache: Arc<ReadOnceBufferCache>,
        offset: usize,
        ssus_version: Option<SendVersion>,
    ) -> anyhow::Result<SendStreamUpgradeSource<'a>> {
        Ok(Self {
            ssus_source: SendStreamSource::SssReadOnceBufferCache(buffer_cache),
            ssus_offset: offset,
            ssus_version,
        })
    }
    pub fn read(&mut self, buffer: &mut [u8]) -> anyhow::Result<usize> {
        let mut total_bytes_read = 0;
        match self.ssus_source {
            SendStreamSource::SssBufReader(ref mut reader) => {
                while total_bytes_read < buffer.len() {
                    // Try to read some data
                    let bytes_read = reader.read(&mut buffer[total_bytes_read..])?;
                    // If we read nothing, then assume that we hit the EOF
                    // Time to exit
                    if bytes_read == 0 {
                        break;
                    }
                    total_bytes_read += bytes_read;
                }
            }
            SendStreamSource::SssSliceReader(slice) => {
                // Assume that we can service everything from the given slice
                let end_offset = self.ssus_offset + buffer.len();
                buffer[..].copy_from_slice(&slice[self.ssus_offset..end_offset]);
                total_bytes_read = buffer.len();
            }
            SendStreamSource::SssReadOnceBufferCache(ref mut buffer_cache) => {
                (*buffer_cache).read_exact(buffer, self.ssus_offset)?;
                total_bytes_read = buffer.len();
            }
            SendStreamSource::SssNoSource => anyhow::bail!("Reading from NoSource"),
        }
        self.ssus_offset += total_bytes_read;
        Ok(total_bytes_read)
    }
    pub fn is_external(&self) -> bool {
        match self.ssus_source {
            SendStreamSource::SssBufReader(_) => true,
            _ => false,
        }
    }
    pub fn get_offset(&self) -> usize {
        self.ssus_offset
    }
    pub fn get_length(&self) -> anyhow::Result<usize> {
        match self.ssus_source {
            SendStreamSource::SssSliceReader(slice) => Ok(slice.len()),
            _ => anyhow::bail!("Cannot get length of source!"),
        }
    }
    pub fn get_version(&self) -> anyhow::Result<SendVersion> {
        match self.ssus_version {
            Some(version) => Ok(version),
            None => anyhow::bail!("Source version not set"),
        }
    }
    pub fn adjust_offset(&mut self, increment: usize) -> anyhow::Result<()> {
        match self.ssus_source {
            SendStreamSource::SssReadOnceBufferCache(_) => self.ssus_offset += increment,
            _ => anyhow::bail!("Cannot adjust offset on source!"),
        }
        Ok(())
    }
    pub fn set_offset(&mut self, offset: usize) -> anyhow::Result<()> {
        match self.ssus_source {
            SendStreamSource::SssReadOnceBufferCache(_) => self.ssus_offset = offset,
            _ => anyhow::bail!("Cannot set offset on source!"),
        }
        Ok(())
    }
    pub fn set_version(&mut self, version: SendVersion) {
        self.ssus_version = Some(version);
    }
}
