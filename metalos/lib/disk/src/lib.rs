/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */
#![feature(read_buf)]
#![feature(can_vector)]

use anyhow::{Context, Result};
use delegate::delegate;
use std::fs::{File, OpenOptions};
use std::io::{Read, ReadBuf, Seek, SeekFrom, Write};
use std::os::unix::io::AsRawFd;
use std::path::PathBuf;

pub static MEGABYTE: u64 = 1024 * 1024;

// define ioctl macros based on the codes in linux/fs.h
nix::ioctl_read!(ioctl_blkgetsize64, 0x12, 114, u64);

#[derive(Debug, Clone, PartialEq)]
pub struct DiskDevPath(pub PathBuf);

impl DiskDevPath {
    pub fn open_ro_file(&self) -> Result<DiskFileRO> {
        let file = OpenOptions::new()
            .write(false)
            .read(true)
            .open(&self.0)
            .context("failed to open device")?;
        Ok(DiskFileRO(file))
    }

    pub fn open_rw_file(&mut self) -> Result<DiskFileRW> {
        let file = OpenOptions::new()
            .write(true)
            .read(true)
            .open(&self.0)
            .context("failed to open device")?;
        Ok(DiskFileRW(file))
    }
}

pub struct DiskFileRO(File);
pub struct DiskFileRW(File);

pub trait FileWrapper {}
impl FileWrapper for DiskFileRO {}
impl FileWrapper for DiskFileRW {}

// Private GetFile traits
// So only this lib can get to the raw File
trait GetFile {
    fn get_file(&self) -> &File;
}

trait GetMutFile {
    fn get_mut_file(&mut self) -> &mut File;
}

impl GetFile for DiskFileRO {
    fn get_file(&self) -> &File {
        &self.0
    }
}

impl GetFile for DiskFileRW {
    fn get_file(&self) -> &File {
        &self.0
    }
}

impl GetMutFile for DiskFileRW {
    fn get_mut_file(&mut self) -> &mut File {
        &mut self.0
    }
}

pub trait ReadDisk {
    fn get_block_device_size(&self) -> Result<u64>;
}

pub trait WriteDisk {}

impl<T: GetFile> ReadDisk for T {
    fn get_block_device_size(&self) -> Result<u64> {
        let file: &File = self.get_file();
        let fd = file.as_raw_fd();

        let mut cap = 0u64;
        let cap_ptr = &mut cap as *mut u64;

        unsafe {
            ioctl_blkgetsize64(fd, cap_ptr)?;
        }

        Ok(cap)
    }
}

impl<T: GetMutFile> WriteDisk for T {}

/* File Traits pass-through */

impl Read for DiskFileRO {
    delegate! {
        to self.0 {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
            fn read_buf(&mut self, buf: &mut ReadBuf<'_>) -> std::io::Result<()>;
            fn read_vectored(&mut self, bufs: &mut [std::io::IoSliceMut<'_>]) -> std::io::Result<usize>;
            fn is_read_vectored(&self) -> bool;
            fn read_to_end(&mut self, buf: &mut Vec<u8>) -> std::io::Result<usize>;
            fn read_to_string(&mut self, buf: &mut String) -> std::io::Result<usize>;
            fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()>;
        }
    }
}

impl Read for DiskFileRW {
    delegate! {
        to self.0 {
            fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize>;
            fn read_buf(&mut self, buf: &mut ReadBuf<'_>) -> std::io::Result<()>;
            fn read_vectored(&mut self, bufs: &mut [std::io::IoSliceMut<'_>]) -> std::io::Result<usize>;
            fn is_read_vectored(&self) -> bool;
            fn read_to_end(&mut self, buf: &mut Vec<u8>) -> std::io::Result<usize>;
            fn read_to_string(&mut self, buf: &mut String) -> std::io::Result<usize>;
            fn read_exact(&mut self, buf: &mut [u8]) -> std::io::Result<()>;
        }
    }
}

impl Seek for DiskFileRO {
    delegate! {
        to self.0 {
            fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64>;
        }
    }
}

impl Seek for DiskFileRW {
    delegate! {
        to self.0 {
            fn seek(&mut self, pos: SeekFrom) -> std::io::Result<u64>;
        }
    }
}

impl Write for DiskFileRW {
    delegate! {
        to self.0 {
            fn write_all(&mut self, buf: &[u8]) -> std::io::Result<()>;
            fn write(&mut self, buf: &[u8]) -> std::io::Result<usize>;
            fn write_vectored(&mut self, bufs: &[std::io::IoSlice<'_>]) -> std::io::Result<usize>;
            fn is_write_vectored(&self) -> bool;
            fn flush(&mut self) -> std::io::Result<()>;
        }
    }
}

/* END File Traits pass-through */

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
            disk.open_ro_file()
                .context("Failed to open disk as file")?
                .get_block_device_size()
                .context("Failed to get block device size")?,
            size * 512
        );
        Ok(())
    }
}
