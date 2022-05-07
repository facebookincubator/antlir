use std::fs;
use std::io;
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use bytes::{Buf, Bytes};
use slog::{info, o, warn, Logger};

use expand_partition::{expand_last_partition, PartitionDelta};
use metalos_disk::DiskDevPath;
use metalos_host_configs::packages::GptRootDisk;
use metalos_mount::Mounter;
use package_download::HttpsDownloader;

// define ioctl macros based on the codes in linux/fs.h
nix::ioctl_none!(ioctl_blkrrpart, 0x12, 95);

const SYS_BLOCK: &str = "/sys/block";

pub struct DiskImageSummary {
    pub disk: DiskDevPath,
    pub partition_device: PathBuf,
    pub partition_delta: PartitionDelta,
}

fn rescan_partitions(file: &fs::File) -> Result<()> {
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

fn get_partition_device(
    log: Logger,
    disk_device: &DiskDevPath,
    partition_number: u32,
) -> Result<PathBuf> {
    let filename = disk_device
        .0
        .file_name()
        .context("Provided disk path doesn't have a file name")?
        .to_str()
        .context(format!(
            "Provided path {:?} contained invalid unicode",
            disk_device
        ))?;

    let sys_dir = Path::new(SYS_BLOCK).join(filename);

    let entries =
        fs::read_dir(&sys_dir).context(format!("failed to read sys_dir {:?}", sys_dir))?;
    for entry in entries {
        let path = entry
            .context(format!(
                "failed to read next dir from sys_dir {:?}",
                sys_dir
            ))?
            .path();

        let entry_filename = path
            .file_name()
            .context(format!("failed to get filename for path {:?}", path))?
            .to_str()
            .context(format!("Path {:?} contained invalid unicode", path))?;

        // each partition will have it's own directory with the full name of the partition
        // device that we are looking for. For example /sys/block/vda will have vda1, vda2 etc
        // and /sys/block/nvme0n1/ will have nvme0n1p1, nvme0n1p2 etc.
        if !entry_filename.starts_with(filename) {
            continue;
        }

        // Now that we have a possible block device which could be our target parition we look
        // inside that directory for a file called 'partition' which will contain the partition
        // number.
        let partition_file = path.join("partition");

        // I am not entirely confident for every disk type that we will always find this file
        // there may be some cases where this file doesn't exist so I am going to make this
        // an non-fatal condition and instead we will error out at the bottom if we don't find
        // the target partition. The error might be a bit more vague but I think the resilience
        // to any unexpected files being here is worth it.
        if !partition_file.exists() {
            warn!(
                log,
                "Found path {:?} which looked like a partition directory but it was missing the partition file",
                path,
            );
            continue;
        }

        let content = fs::read_to_string(&partition_file)
            .context(format!("Can't read partition file {:?}", partition_file))?;

        let current_partition_number: u32 = content.trim().parse().context(format!(
            "Failed to parse contents of partition file, found: '{:?}'",
            content
        ))?;

        info!(
            log,
            "Found partition {} at {:?}", current_partition_number, path
        );
        if current_partition_number == partition_number {
            return Ok(Path::new("/dev/").join(entry_filename));
        }
    }

    Err(anyhow!(
        "Unable to find partition {} in {:?}",
        partition_number,
        sys_dir
    ))
}

async fn download_disk_image(package: GptRootDisk) -> Result<Bytes> {
    let dl = HttpsDownloader::new().context("while creating downloader")?;
    let url = dl.package_url(&package.into());
    let client: reqwest::Client = dl.into();
    client
        .get(url.clone())
        .send()
        .await
        .with_context(|| format!("while opening {}", url))?
        .bytes()
        .await
        .with_context(|| format!("while reading {}", url))
}

pub async fn apply_disk_image<M: Mounter>(
    log: Logger,
    disk: DiskDevPath,
    package: GptRootDisk,
    tmp_mounts_dir: &Path,
    dd_buffer_size: usize,
    mounter: M,
) -> Result<DiskImageSummary> {
    let log = log.new(o!("package" => format!("{:?}", package)));
    let src = download_disk_image(package).await?;
    let src_len = src.len();
    let mut src = src.reader();

    let mut dst = fs::OpenOptions::new()
        .write(true)
        .open(&disk.0)
        .context("Failed to open destination file")?;

    let mut buf = vec![0; dd_buffer_size];
    loop {
        let len = match src.read(&mut buf) {
            Ok(0) => break,
            Ok(len) => len,
            Err(ref e) if e.kind() == io::ErrorKind::Interrupted => continue,
            Err(e) => {
                return Err(e).context("Failed to read next block from file");
            }
        };
        dst.write_all(&buf[..len])
            .context("Failed to write next block to file")?;
    }
    info!(log, "Wrote {} bytes to {:?}", src_len, disk);

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

    let partition_dev =
        get_partition_device(log.clone(), &disk, delta.partition_num).context(format!(
            "Failed to get partition {} of device {:?}",
            delta.partition_num, disk,
        ))?;

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
//
#[cfg(test)]
mod tests {
    use super::*;
    use expand_partition::test_utils::*;
    use metalos_macros::vmtest;
    use slog::o;

    #[vmtest]
    fn test_get_partition_device() -> Result<()> {
        let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());

        let (lo, _) = setup_test_device().context("failed to setup loopback device")?;

        let mut part =
            lo.0.to_str()
                .context("Failed to convert disk dev path to str")?
                .to_string();
        part.push_str("p3");
        assert_eq!(
            get_partition_device(log, &lo, 3).context("Failed to get partition device")?,
            PathBuf::from(&part)
        );

        Ok(())
    }
}
