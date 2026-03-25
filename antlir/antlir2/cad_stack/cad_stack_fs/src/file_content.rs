/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::os::fd::AsRawFd;

use cad_stack::Checksum;
use cad_stack::Object;
use cad_stack::Result;

pub struct FileContent {
    file: File,
}

impl FileContent {
    /// Create a FileContent from a cap_std File (converted to std File).
    pub fn from_cap_std_file(file: File) -> Self {
        Self { file }
    }

    /// Write the file content to the given output file.
    pub fn write_to(mut self, output: &mut File) -> std::io::Result<()> {
        std::io::copy(&mut self.file, output)?;
        Ok(())
    }
}

impl Object for FileContent {
    fn from_file(file: File) -> Result<Self> {
        Ok(Self { file })
    }

    fn to_file(&self, file: &mut File) -> Result<()> {
        std::io::copy(&mut (&self.file), file)?;
        Ok(())
    }

    fn checksum(&self) -> Result<Checksum<Self>> {
        let mut hasher = blake3::Hasher::new();
        hasher.update_mmap_rayon(format!("/proc/self/fd/{}", self.file.as_raw_fd()))?;
        Ok(Checksum::new(hasher.finalize()))
    }
}
