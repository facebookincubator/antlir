/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::fs::OpenOptions;
use std::io::BufWriter;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::PathBuf;

use crate::send_elements::send_version::SendVersion;

enum SendStreamDestination<'a> {
    /// A BufWriter destination for IO to stdout or a file
    SsdBufWriter(BufWriter<Box<dyn Write + Send>>),
    /// A slice destination
    SsdSliceWriter(&'a mut [u8]),
    /// No destination at all
    SsdNoDestination,
}

pub struct SendStreamUpgradeDestination<'a> {
    /// The destination for the send stream data
    ssud_destination: SendStreamDestination<'a>,
    /// An offset into the send stream data destination
    ssud_offset: usize,
    /// The version of the send stream data
    ssud_version: Option<SendVersion>,
}

impl<'a> SendStreamUpgradeDestination<'a> {
    pub fn new_from_file(
        output: Option<PathBuf>,
        write_buffer_size: usize,
        offset: usize,
        version: Option<SendVersion>,
    ) -> anyhow::Result<Self> {
        // Open up the output file
        let output_file: Box<dyn Write + Send + Sync> = match output {
            // Ignore the seek for stdout -- this is a pipe, and the stream
            // will be at the right spot
            None => Box::new(std::io::stdout()),
            Some(ref value) => {
                let mut file: File;
                if offset != 0 {
                    file = OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(value)?;
                } else {
                    // NOTE: Unlike in the new case, don't truncate the file
                    // since we're reopening it
                    file = OpenOptions::new().write(true).open(value)?;
                    // If we had a file input, then we need to seek forward
                    // on the underlying file
                    file.seek(SeekFrom::Start(offset as u64))?;
                }
                Box::new(file)
            }
        };
        Ok(Self {
            ssud_destination: SendStreamDestination::SsdBufWriter(BufWriter::with_capacity(
                write_buffer_size,
                output_file,
            )),
            ssud_offset: offset,
            ssud_version: version,
        })
    }
    pub fn new_from_slice(
        slice: Option<&'a mut [u8]>,
        offset: usize,
        version: Option<SendVersion>,
    ) -> SendStreamUpgradeDestination<'a> {
        let destination = match slice {
            Some(s) => SendStreamDestination::SsdSliceWriter(s),
            None => SendStreamDestination::SsdNoDestination,
        };
        Self {
            ssud_destination: destination,
            ssud_offset: offset,
            ssud_version: version,
        }
    }
    pub fn new_from_none(
        version: Option<SendVersion>,
    ) -> anyhow::Result<SendStreamUpgradeDestination<'a>> {
        Ok(Self {
            ssud_destination: SendStreamDestination::SsdNoDestination,
            ssud_offset: 0,
            ssud_version: version,
        })
    }
    pub fn write_all(&mut self, buffer: &[u8]) -> anyhow::Result<()> {
        match self.ssud_destination {
            SendStreamDestination::SsdBufWriter(ref mut writer) => writer.write_all(buffer)?,
            SendStreamDestination::SsdSliceWriter(ref mut slice) => {
                // Assume that we can service everything from the given slice
                let end_offset = self.ssud_offset + buffer.len();
                slice[self.ssud_offset..end_offset].copy_from_slice(buffer);
            }
            SendStreamDestination::SsdNoDestination => anyhow::bail!("Writing to NoDestination"),
        }
        self.ssud_offset += buffer.len();
        Ok(())
    }
    pub fn get_offset(&self) -> usize {
        self.ssud_offset
    }
    pub fn get_length(&self) -> anyhow::Result<usize> {
        match self.ssud_destination {
            SendStreamDestination::SsdSliceWriter(ref slice) => Ok(slice.len()),
            _ => anyhow::bail!("Cannot get length of destination!"),
        }
    }
    pub fn get_version(&self) -> anyhow::Result<SendVersion> {
        match self.ssud_version {
            Some(version) => Ok(version),
            None => anyhow::bail!("Destination version not set"),
        }
    }
    pub fn flush(&mut self) -> anyhow::Result<()> {
        match self.ssud_destination {
            SendStreamDestination::SsdBufWriter(ref mut writer) => match writer.flush() {
                Ok(()) => Ok(()),
                Err(error) => anyhow::bail!(error),
            },
            _ => anyhow::bail!("Cannot flush destination!"),
        }
    }
    pub fn set_version(&mut self, version: SendVersion) {
        self.ssud_version = Some(version);
    }
}
