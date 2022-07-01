/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::anyhow;
use anyhow::Context;
use anyhow::Result;
use metalos_disk::DiskDevPath;
use metalos_disk::DiskFileRW;
use metalos_disk::ReadDisk;
use metalos_disk::MEGABYTE;
use rand::rngs::SmallRng;
use rand::RngCore;
use rand::SeedableRng;
use std::io::Read;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;

/**
 * This function wipes the partition table & backup of a block device
 * Additionally, it performs some sanity checks on the device.
 *
 * This is convenient for recreating partition tables, but *NOT* meant
 * as sanitisation of the disk.
 */
pub fn quick_wipe_disk(disk: &mut DiskDevPath) -> Result<()> {
    // Get 1MB of random data
    let rand_buf = {
        let mut buf = [0; 1024 * 1024];
        let mut small_rng = SmallRng::from_entropy();
        small_rng.fill_bytes(&mut buf);
        buf
    };

    // Write 1MB of random data to disk
    {
        let mut disk_file = disk.open_rw_file()?;
        write_mb_buf(&mut disk_file, SeekFrom::Start(0), 1, &rand_buf)?;
    }

    // Re-open disk, and read the data back to verify that the system is healthy
    // enough to make it past partitioning
    {
        let mut disk_file = disk.open_rw_file()?;
        let mut read_buf = [0; 1024 * 1024];
        disk_file.read_exact(&mut read_buf)?;
        if read_buf != rand_buf {
            return Err(anyhow!(
                "When reading the disk, it returns different data than has been written to it. This is likely an indication of hardware failure."
            ));
        }

        // Overwrite the beginning & end of the disk
        overwrite_beginning_end_disk(&mut disk_file);
    }

    // Re-open disk, and verify that the first 64M are empty
    {
        let mut disk_file = disk.open_rw_file()?;
        check_empty(&mut disk_file, SeekFrom::Start(0), 64);
    }

    Ok(())
}

fn overwrite_beginning_end_disk(disk_file: &mut DiskFileRW) -> Result<()> {
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

    Ok(())
}

fn write_mb_buf(
    disk_file: &mut DiskFileRW,
    start: SeekFrom,
    megabytes: u64,
    buf: &[u8; 1024 * 1024],
) -> Result<()> {
    disk_file.seek(start)?;
    for _ in 0..megabytes {
        disk_file.write_all(buf).context("Failed to write buffer")?;
    }
    disk_file.flush()?;

    Ok(())
}

fn check_empty(disk_file: &mut DiskFileRW, start: SeekFrom, megabytes: u64) -> Result<()> {
    let empty_buf = [0; 1024 * 1024];
    let mut out_buf = [0; 1024 * 1024];
    disk_file.seek(start)?;
    for _ in 0..megabytes {
        disk_file
            .read_exact(&mut out_buf)
            .context("Failed to read buffer from disk")?;
        if (empty_buf != out_buf) {
            return Err(anyhow!("Disk block should be empty but isn't"));
        }
    }

    Ok(())
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use metalos_macros::vmtest;

    fn write_random(disk_file: &mut DiskFileRW, start: SeekFrom, megabytes: u64) -> Result<()> {
        let rand_buf = {
            let mut buf = [0; 1024 * 1024];
            SmallRng::from_entropy().fill_bytes(&mut buf);
            buf
        };
        write_mb_buf(disk_file, start, megabytes, &rand_buf)
    }

    #[vmtest]
    fn test_quick_wipe_disk() -> Result<()> {
        // Open disk
        let mut disk = DiskDevPath("/dev/vda".into());
        let mut disk_file = disk.open_rw_file()?;

        // Make sure areas are not empty before wipe
        write_random(&mut disk_file, SeekFrom::Start(0), 64)?;
        write_random(&mut disk_file, SeekFrom::End((MEGABYTE as i64) * -64), 64)?;

        // Wipe
        quick_wipe_disk(&mut disk)?;

        // Check whether the expected area's are zeroes
        check_empty(&mut disk_file, SeekFrom::Start(0), 64)?;
        check_empty(&mut disk_file, SeekFrom::End((MEGABYTE as i64) * -64), 64)?;

        Ok(())
    }
}
