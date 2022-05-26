use std::fs;
use std::fs::File;
use std::io::Write;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use bytes::Bytes;
use futures::StreamExt;
use slog::{info, o, warn, Logger};

use expand_partition::{expand_last_partition, PartitionDelta};
use metalos_disk::{scan_disk_partitions, DiskDevPath};
use metalos_host_configs::packages::GptRootDisk;
use metalos_mount::Mounter;
use package_download::{HttpsDownloader, PackageDownloader};

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
    rescan_partitions(&dst).context("Failed to rescan partitions after writing image")?;

    let partition_devs = scan_disk_partitions(log.clone(), &disk)
        .context(format!("failed to scan partitions for {:?}", disk))?;

    let partition_dev = match partition_devs.get(&delta.partition_num) {
        Some(dev) => dev.clone(),
        None => {
            return Err(anyhow!(
                "Unable to find partition {} for {:?}",
                delta.partition_num,
                disk
            ));
        }
    };

    info!(log, "Expanding filesystem in {:?}", partition_dev);
    expand_filesystem(&partition_dev, tmp_mounts_dir, mounter)
        .context("Failed to expand filesystem after expanding partition")?;

    Ok(DiskImageSummary {
        partition_device: partition_dev,
        partition_delta: delta,
        disk,
    })
}

// TODO(T108026401): We want to test this on it's own however we need multiple disks
// first in VMtest. I tried to test this with loopbacks, the rootfs and inside the
// initrd but each of those had their own blockers and were kind of hacks anyway.
// so for now the coverage comes from the end-to-end test (switch-root-reimage)
