/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::fs::{File, OpenOptions};

use anyhow::{Context, Result};
use gpt::disk::LogicalBlockSize;
use gpt::header::read_header_from_arbitrary_device;
use gpt::partition::{file_read_partitions, Partition};

use metalos_disk::{DiskDevPath, DiskFile};

static MEGABYTE: u64 = 1024 * 1024;

#[derive(Debug)]
pub struct PartitionDelta {
    pub partition_num: u32,
    pub old_size: u64,
    pub new_size: u64,
    pub new_last_lb: u64,
}

pub fn expand_last_partition(device: &DiskDevPath) -> Result<PartitionDelta> {
    // First we read the current device GPT header and it's partitions.
    // We can't use the top level GptConfig logic from the crate because that
    // assumes that the backup is in the correct place which it won't necessarily be
    // because we have just dd'd the image to this disk.
    let mut disk_file = device.open_as_file()?;

    let (lb_size, primary_header) =
        match read_header_from_arbitrary_device(&mut disk_file.0, LogicalBlockSize::Lb512) {
            Ok(header) => Ok((LogicalBlockSize::Lb512, header)),
            Err(e) => {
                match read_header_from_arbitrary_device(&mut disk_file.0, LogicalBlockSize::Lb4096)
                {
                    Ok(header) => Ok((LogicalBlockSize::Lb4096, header)),
                    Err(_) => Err(e),
                }
            }
        }
        .context("Failed to read the primary header from disk")?;

    let original_partitions = file_read_partitions(&mut disk_file.0, &primary_header, lb_size)
        .context("failed to read partitions from disk_device file")?;

    // Now we must find the end of the disk that we are allowed to expand up to and transform our
    // partitions so that the last one goes all the way to the end
    let (new_partitions, delta) = transform_partitions(
        original_partitions.clone(),
        lb_size,
        get_last_usable_lb(&disk_file, lb_size)
            .context("failed to find last usable block of device")?,
    )
    .context("failed to transform partitions")?;

    // Finally in order to get the final setup valid we must write a whole new GPT so that the backup
    // will be in the right place.
    let mut new_gpt_table = gpt::GptConfig::new()
        .writable(true)
        .initialized(false)
        .logical_block_size(lb_size)
        .open(&device.0)
        .context("failed to load gpt table")?;

    new_gpt_table
        .update_guid(Some(primary_header.disk_guid))
        .context("failed to copy over guid")?;

    new_gpt_table
        .update_partitions(new_partitions)
        .context("failed to add updated partitions to gpt_table")?;

    let device = new_gpt_table
        .write()
        .context("failed to write updated table")?;

    // Now we double check that all wen't well by trying to load back the GPT using the high level
    // API that enforces the backups are valid.
    gpt::GptConfig::new()
        .writable(false)
        .initialized(true)
        .open_from_device(device)
        .context("failed to read GPT after resize")?;

    Ok(delta)
}

fn transform_partitions(
    mut partitions: BTreeMap<u32, Partition>,
    lb_size: LogicalBlockSize,
    last_usable_lba: u64,
) -> Result<(BTreeMap<u32, Partition>, PartitionDelta)> {
    let (last_partition_id, mut last_partition) = partitions
        .iter_mut()
        .max_by_key(|(_, p)| p.last_lba)
        .context("Failed to find the last partition")?;

    let original_last_lba = last_partition.last_lba;
    last_partition.last_lba = last_usable_lba;

    let lb_size_bytes: u64 = lb_size.into();
    let delta = PartitionDelta {
        partition_num: *last_partition_id,
        old_size: (original_last_lba - last_partition.first_lba) * lb_size_bytes,
        new_size: (last_partition.last_lba - last_partition.first_lba) * lb_size_bytes,
        new_last_lb: last_partition.last_lba,
    };
    Ok((partitions, delta))
}

fn get_last_usable_lb(disk_file: &DiskFile, lb_size: LogicalBlockSize) -> Result<u64> {
    let lb_size_bytes: u64 = lb_size.clone().into();
    let disk_size = disk_file
        .get_block_device_size()
        .context("Failed to find disk size")?;

    // I am not sure why this is the forumla. I copied it from D26917298
    // I believe it has something to do with making sure that the last lb lies on a MB
    // boundary
    Ok(((disk_size - MEGABYTE) / lb_size_bytes) - 1)
}

pub mod test_utils {
    use crate::DiskDevPath;
    use anyhow::{Context, Result};

    pub fn setup_test_loopback(img_file: &str) -> Result<String> {
        std::process::Command::new("dd")
            .args(&[
                "if=/dev/zero",
                &format!("of={}", img_file),
                "bs=512",
                "count=100000",
            ])
            .output()
            .context(format!("Failed to run dd to make {}", img_file))?;

        let output = std::process::Command::new("losetup")
            .arg("-f")
            .arg("--show")
            .arg(&img_file)
            .output()
            .context(format!("Failed to run dd to make /tmp/{}", img_file))?;
        println!("losetup output: {:?}", output);
        assert!(output.status.success());

        let lo = std::str::from_utf8(&output.stdout)
            .context(format!("Invalid UTF-8 in output: {:?}", output))?
            .trim();

        println!("lo: {}", lo);
        assert!(lo.starts_with("/dev/loop"));
        Ok(lo.to_string())
    }

    pub fn setup_test_device() -> Result<(DiskDevPath, String)> {
        let img_file = "/tmp/loopbackfile.img".to_string();
        let lo = setup_test_loopback(&img_file).context("Failed to setup loopback device")?;
        let output = std::process::Command::new("parted")
            .args(&["--script", &lo, "mklabel", "gpt"])
            .output()
            .context("Failed to make gpt label")?;
        println!("mklabel: {:?}", output);
        assert!(output.status.success());

        let output = std::process::Command::new("parted")
            .args(&["--script", &lo, "mkpart", "primary", "btrfs", "50s", "100s"])
            .output()
            .context("Failed to make p1")?;
        println!("p1: {:?}", output);
        assert!(output.status.success());

        let output = std::process::Command::new("parted")
            .args(&[
                "--script", &lo, "mkpart", "primary", "btrfs", "201s", "800s",
            ])
            .output()
            .context("Failed to make p2")?;
        println!("p2: {:?}", output);
        assert!(output.status.success());

        let output = std::process::Command::new("parted")
            .args(&[
                "--script", &lo, "mkpart", "primary", "btrfs", "101s", "200s",
            ])
            .output()
            .context("Failed to make p3")?;
        println!("p3: {:?}", output);
        assert!(output.status.success());

        let output = std::process::Command::new("fdisk")
            .args(&["-l", &lo])
            .output()
            .context("Failed to run fdisk")?;

        println!("{:#?}", output);
        assert!(output.status.success());

        let output = std::process::Command::new("sync")
            .output()
            .context("Failed to run sync")?;
        println!("{:#?}", output);
        assert!(output.status.success());

        Ok((DiskDevPath(lo.into()), img_file))
    }
}

#[cfg(test)]
pub mod tests {
    use super::test_utils::*;
    use super::*;
    use metalos_macros::vmtest;

    fn get_guid(disk_path: &DiskDevPath) -> Result<String> {
        let cfg = gpt::GptConfig::new().writable(false);
        let disk = cfg.open(&disk_path.0).context("failed to open disk")?;

        Ok(disk.guid().to_hyphenated_ref().to_string())
    }

    #[vmtest]
    fn test_expand_last_partition() -> Result<()> {
        let (lo, _) = setup_test_device().context("failed to setup loopback device")?;
        let start_guid = get_guid(&lo).context("failed to get starting guid")?;
        let delta = expand_last_partition(&lo).context("failed to expand last partition")?;
        let ending_guid = get_guid(&lo).context("failed to get starting guid")?;

        println!("{:#?}", delta);
        assert_eq!(delta.partition_num, 2);
        assert_eq!(delta.old_size, 599 * 512);

        // Entire disk should be 51200000 bytes or 100000 sectors we reserve up to
        // 1MB with the formula ((51200000 - (1024 * 1024)) / 512) - 1 = 97951
        assert_eq!(delta.new_last_lb, 97951);

        // start of 3rd part should be sector 201 (102912 bytes).and with the new end
        // at 97951 (50150912) so end size should be 50150912 - 102912 = 50048000
        assert_eq!(delta.new_size, 50048000);

        // Ensure this conversion didn't mess up the guid
        assert_eq!(start_guid, ending_guid);

        Ok(())
    }
}
