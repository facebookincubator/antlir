use std::fs;
use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use futures::{future, StreamExt};
use slog::{info, o, Logger};
use tokio::time::timeout;

use expand_partition::{expand_last_partition, PartitionDelta};
use metalos_disk::DiskDevPath;
use metalos_host_configs::packages::GptRootDisk;
use metalos_mount::Mounter;
use package_download::{HttpsDownloader, PackageDownloader};
use udev_utils::device::{Disk, SpecificDevice};

// define ioctl macros based on the codes in linux/fs.h
nix::ioctl_none!(ioctl_blkrrpart, 0x12, 95);

pub struct DiskImageSummary {
    pub disk: DiskDevPath,
    pub partition_device: PathBuf,
    pub partition_delta: PartitionDelta,
}

pub fn rescan_partitions(file: &fs::File) -> Result<()> {
    let fd = file.as_raw_fd();
    unsafe {
        ioctl_blkrrpart(fd)?;
    }
    Ok(())
}

fn expand_filesystem<M: Mounter>(device: &Path, mount_path: &Path, mounter: M) -> Result<()> {
    if !mount_path.exists() {
        fs::create_dir_all(mount_path).context("failed to create temporary mount directory")?;
    }

    mounter
        .mount(
            device,
            mount_path,
            Some("btrfs"),
            nix::mount::MsFlags::empty(),
            None,
        )
        .context(format!(
            "failed to mount root partition {:?} to {:?}",
            device, mount_path,
        ))?;

    let output = Command::new("btrfs")
        .arg("filesystem")
        .arg("resize")
        .arg("max")
        .arg(mount_path)
        .output()
        .context(format!("Failed to run btrfs resize max {:?}", mount_path))?;

    if !output.status.success() {
        return Err(anyhow!(
            "btrfs filesystem resize max {:?} failed: {:?}",
            mount_path,
            output
        ));
    }

    mounter
        .umount(mount_path, false)
        .context(format!("Failed to umount {:?}", mount_path))
}

async fn download_disk_image(log: Logger, package: GptRootDisk, dest: &mut File) -> Result<usize> {
    let package = metalos_host_configs::packages::generic::Package::from(package);
    let dl = HttpsDownloader::new().context("while creating downloader")?;
    let mut stream = (&dl).open_bytes_stream(log.clone(), &package).await?;

    let mut size = 0;
    while let Some(item) = stream.next().await {
        let bytes: Bytes = item.context("while reading chunk from downloader")?;
        size += bytes.len();

        dest.write_all(&bytes)
            .context("while writing chunk to disk")?;
    }

    Ok(size)
}

pub async fn apply_disk_image<M: Mounter>(
    log: Logger,
    disk: DiskDevPath,
    package: GptRootDisk,
    tmp_mounts_dir: &Path,
    mounter: M,
) -> Result<DiskImageSummary> {
    let log = log.new(o!("package" => format!("{:?}", package)));

    let mut dst = fs::OpenOptions::new()
        .write(true)
        .open(&disk.0)
        .context("Failed to open destination file")?;

    info!(log, "downloading {:?} to {}", package, disk.0.display());
    let bytes_written = download_disk_image(log.clone(), package, &mut dst)
        .await
        .context("Failed to write disk image")?;

    info!(log, "Wrote {} bytes to {:?}", bytes_written, disk);
    info!(log, "Expanding last partition of {:?}", disk);
    let delta = expand_last_partition(&disk)
        .context(format!("Failed to expand last partition of: {:?}", disk))?;
    info!(
        log,
        "Expanded {:?} partition {} from {} bytes to {} bytes with new last lba = {}",
        disk,
        delta.partition_num,
        delta.old_size,
        delta.new_size,
        delta.new_last_lb,
    );

    if unsafe { libc::syncfs(dst.as_raw_fd()) } != 0 {
        return Err(nix::Error::last()).context("Failed to run syncfs");
    }

    info!(log, "Rescanning partition table for {:?}", disk);

    // wait for udev to load the device info for the partition number we are
    // looking for
    let disk_device = Disk::from_path(&disk.0).with_context(|| {
        format!(
            "while creating udev_utils::device::Disk for {}",
            disk.0.display()
        )
    })?;

    rescan_partitions(&dst).context("Failed to rescan partitions after writing image")?;

    let partition_dev: PathBuf = timeout(
        Duration::from_secs(1),
        udev_utils::stream(Default::default())
            .await
            .context("while creating udev stream for")?
            .filter_map(|event| future::ready(event.into_attached_device()))
            .filter_map(|dev| future::ready(udev_utils::device::Partition::try_from(dev).ok()))
            .filter(|part| future::ready(part.parent().map_or(false, |p| *p == disk_device)))
            .filter(|part| future::ready(part.number() == delta.partition_num))
            .map(|part| part.path().to_path_buf())
            .next(),
    )
    .await
    .with_context(|| {
        format!(
            "Partition {} on {} did not show up within 1s",
            delta.partition_num,
            disk.0.display()
        )
    })?
    .with_context(|| {
        format!(
            "Partition {} on {} never appeared",
            delta.partition_num,
            disk.0.display()
        )
    })?;

    info!(log, "Expanding filesystem in {:?}", partition_dev);
    expand_filesystem(&partition_dev, tmp_mounts_dir, mounter)
        .context("Failed to expand filesystem after expanding partition")?;

    // this is awful, but for some reason the partition device frequently
    // disappears after expanding the fs, then it comes back again
    // sleep a little bit to make sure that it has time to disappear
    tokio::time::sleep(Duration::from_secs(5)).await;
    for _ in 0..10 {
        if partition_dev.exists() {
            return Ok(DiskImageSummary {
                partition_device: partition_dev.clone(),
                partition_delta: delta,
                disk,
            });
        } else {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
    Err(anyhow!("{:?} has vanished", partition_dev))
}

// TODO(T108026401): We want to test this on it's own however we need multiple disks
// first in VMtest. I tried to test this with loopbacks, the rootfs and inside the
// initrd but each of those had their own blockers and were kind of hacks anyway.
// so for now the coverage comes from the end-to-end test (switch-root-reimage)
