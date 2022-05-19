/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::{anyhow, Context, Result};
use metalos_disk::{DiskFileRW, ReadDisk, MEGABYTE};
use std::io::{Seek, SeekFrom, Write};

/**
 * This function deletes the partition table & backup of a block device
 * by zeroing out the first & last 64M of the device.
 *
 * This is convenient for recreating partition tables, but *NOT* meant
 * as sanitisation of the disk.
 */
pub fn lazy_wipe(mut disk_file: DiskFileRW) -> Result<DiskFileRW> {
    let size: u64 = disk_file.get_block_device_size()?;
    if size < (MEGABYTE * 64) {
        return Err(anyhow!("Expected disk size > 64M"));
    }
    let empty_mb_buf = [0; 1024 * 1024];

    // Wipe first 64M
    disk_file.seek(SeekFrom::Start(0))?;
    for _ in 0..64 {
        disk_file
            .write_all(&empty_mb_buf)
            .context("Failed to write zeroes to beginning of disk")?
    }

    // Wipe last 64M
    disk_file.seek(SeekFrom::End((MEGABYTE as i64) * -64))?;
    for _ in 0..64 {
        disk_file
            .write_all(&empty_mb_buf)
            .context("Failed to write zeroes to end of disk")?
    }

    // Flush & Seek back to 0
    disk_file.flush()?;
    disk_file.seek(SeekFrom::Start(0))?;

    Ok(disk_file)
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use metalos_disk::DiskDevPath;
    use metalos_macros::vmtest;
    use rand::*;
    use std::io::Read;

    fn check_empty(disk_file: &mut DiskFileRW, start: SeekFrom, megabytes: u64) -> Result<()> {
        let empty_buf = [0; 1024 * 1024];
        let mut out_buf = [0; 1024 * 1024];
        // let mut file = &disk_file.0;
        disk_file.seek(start)?;
        for _ in 0..megabytes {
            disk_file
                .read_exact(&mut out_buf)
                .context("Failed to read buffer from disk")?;
            assert_eq!(
                empty_buf, out_buf,
                "1MB Disk block should be empty, but isn't!"
            )
        }

        Ok(())
    }

    fn write_random(disk_file: &mut DiskFileRW, start: SeekFrom, megabytes: u64) -> Result<()> {
        let mut mb_buf = [0; 1024 * 1024];
        rand::thread_rng().fill_bytes(&mut mb_buf);

        // Write random characters
        disk_file.seek(start)?;
        for _ in 0..megabytes {
            disk_file
                .write_all(&mb_buf)
                .context("Failed to write random characters")?
        }

        Ok(())
    }

    #[vmtest]
    fn test_lazy_wipe() -> Result<()> {
        // Open disk
        let mut disk = DiskDevPath("/dev/vda".into());
        let mut disk_file = disk.open_rw_file()?;

        // Make sure areas are not empty before wipe
        write_random(&mut disk_file, SeekFrom::Start(0), 64)?;
        write_random(&mut disk_file, SeekFrom::End((MEGABYTE as i64) * -64), 64)?;

        // Wipe
        disk_file = lazy_wipe(disk_file)?;

        // Check whether the expected area's are zeroes
        check_empty(&mut disk_file, SeekFrom::Start(0), 64)?;
        check_empty(&mut disk_file, SeekFrom::End((MEGABYTE as i64) * -64), 64)?;

        Ok(())
    }
}
