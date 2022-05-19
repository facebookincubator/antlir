/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{Context, Result};
use std::fs::{File, OpenOptions};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

// define ioctl macros based on the codes in linux/fs.h
nix::ioctl_read!(ioctl_blkgetsize64, 0x12, 114, u64);

#[derive(Debug, Clone, PartialEq)]
pub struct DiskDevPath(pub PathBuf);

impl DiskDevPath {
    pub fn open_as_file(&self) -> Result<DiskFile> {
        let file = OpenOptions::new()
            .write(false)
            .read(true)
            .open(&self.0)
            .context("failed to open device")?;
        Ok(DiskFile(file))
    }
}

pub struct DiskFile(pub File);

impl DiskFile {
    pub fn get_block_device_size(&self) -> Result<u64> {
        let fd = self.0.as_raw_fd();

        let mut cap = 0u64;
        let cap_ptr = &mut cap as *mut u64;

        unsafe {
            ioctl_blkgetsize64(fd, cap_ptr)?;
        }

        Ok(cap)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use crate::DiskDevPath;
    use metalos_macros::vmtest;

    #[vmtest]
    fn test_get_block_device_size() -> Result<()> {
        let disk = DiskDevPath("/dev/vda".into());

        let output = std::process::Command::new("cat")
            .args(&["/sys/block/vda/size"])
            .output()
            .context("Failed to run cat /sys/block/vda/size")?;

        let size: u64 = std::str::from_utf8(&output.stdout)
            .context(format!("Invalid UTF-8 in output: {:?}", output))?
            .trim()
            .parse()
            .context(format!("Failed to parse output {:?} as u64", output))?;

        assert_eq!(
            disk.open_as_file()
                .context("Failed to open disk as file")?
                .get_block_device_size()
                .context("Failed to get block device size")?,
            size * 512
        );
        Ok(())
    }
}
