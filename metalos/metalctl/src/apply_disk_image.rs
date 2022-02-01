use std::fs;
use std::io;
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use bytes::{Buf, Bytes};
use slog::{info, o, warn, Logger};
use structopt::StructOpt;

use expand_partition::expand_last_partition;
use find_root_disk::{DiskPath, FindRootDisk, SingleDiskFinder};
use image::download::HttpsDownloader;
use image::AnyImage;

// define ioctl macros based on the codes in linux/fs.h
nix::ioctl_none!(ioctl_blkrrpart, 0x12, 95);

const SYS_BLOCK: &str = "/sys/block";

#[derive(StructOpt)]
pub struct Opts {
    package: String,

    #[structopt(long, parse(from_os_str), default_value = "/tmp/expand_root_mnt")]
    tmp_mounts_dir: PathBuf,

    /// Size of the read/write buffer to use in bytes
    /// defaults to the same as real dd
    #[structopt(default_value = "512")]
    buffer_size: usize,
}

fn rescan_partitions(file: &fs::File) -> Result<()> {
    let fd = file.as_raw_fd();
    unsafe {
        ioctl_blkrrpart(fd)?;
    }
    Ok(())
}

pub fn expand_filesystem(device: &Path, mount_path: PathBuf) -> Result<()> {
    if !mount_path.exists() {
        fs::create_dir_all(&mount_path).context("failed to create temporary mount directory")?;
    }

    let output = Command::new("mount")
        .arg(device)
        .arg("-t")
        .arg("btrfs")
        .arg(&mount_path)
        .output()
        .context(format!("Failed to mount {:?} to {:?}", device, mount_path))?;

    if !output.status.success() {
        return Err(anyhow!(
            "mount of {:?} to {:?} failed: {:?}",
            device,
            mount_path,
            output
        ));
    }

    let output = Command::new("btrfs")
        .args(&[
            &"filesystem".into(),
            &"resize".into(),
            &"max".into(),
            &mount_path,
        ])
        .output()
        .context(format!("Failed to run btrfs resize max {:?}", mount_path))?;

    if !output.status.success() {
        return Err(anyhow!(
            "btrfs filesystem resize max {:?} failed: {:?}",
            mount_path,
            output
        ));
    }

    let output = Command::new("umount")
        .args(&[&mount_path])
        .output()
        .context(format!("Failed to umount {:?}", mount_path))?;

    if !output.status.success() {
        return Err(anyhow!("umount {:?} failed: {:?}", mount_path, output));
    }

    Ok(())
}

pub fn get_partition_device(
    log: Logger,
    disk_device: &Path,
    partition_number: u32,
) -> Result<PathBuf> {
    let filename = disk_device
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

fn find_root_disk<FD: FindRootDisk>(disk_finder: &FD) -> Result<PathBuf> {
    disk_finder
        .get_root_device()
        .context("Failed to find root device to write root_disk_package to")?
        .dev_node()
        .context("Failed to get the devnode for root disk")
}

async fn download_disk_image(config: crate::Config, package: String) -> Result<Bytes> {
    // TODO: make this an image::Image all the way through (add it to HostConfig)
    let (name, id) = package
        .split_once(':')
        .context("package must have ':' separator")?;
    let image: AnyImage = package_manifest::types::Image {
        name: name.into(),
        id: id.into(),
        kind: package_manifest::types::Kind::GPT_ROOTDISK,
    }
    .try_into()
    .context("converting image representation")?;

    let dl = HttpsDownloader::new(config.download.package_format_uri().to_string())
        .context("while creating downloader")?;
    let url = dl.image_url(&image).context("while getting image url")?;
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

pub async fn apply_disk_image(log: Logger, opts: Opts, config: crate::Config) -> Result<()> {
    let log = log.new(o!("package" => opts.package.clone()));
    let src = download_disk_image(config, opts.package).await?;
    let src_len = src.len();
    let mut src = src.reader();

    let dest = find_root_disk(&SingleDiskFinder::new()).context("Failed to get root disk")?;
    info!(log, "Selected {:?} as root disk", dest);
    let mut dst = fs::OpenOptions::new()
        .write(true)
        .open(&dest)
        .context("Failed to open destination file")?;

    let mut buf = vec![0; opts.buffer_size];
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
    info!(log, "Wrote {} bytes to {:?}", src_len, dest);

    info!(log, "Expanding last partition of {:?}", dest);
    let delta = expand_last_partition(&dest)
        .context(format!("Failed to expand last partition of: {:?}", dest))?;
    info!(
        log,
        "Expanded {:?} partition {} from {} bytes to {} bytes with new last lba = {}",
        dest,
        delta.partition_num,
        delta.old_size,
        delta.new_size,
        delta.new_last_lb,
    );

    if unsafe { libc::syncfs(dst.as_raw_fd()) } != 0 {
        return Err(nix::Error::last()).context("Failed to run syncfs");
    }

    info!(log, "Rescanning partition table for {:?}", dest);
    rescan_partitions(&dst).context("Failed to rescan partitions after writing image")?;

    let partition_dev =
        get_partition_device(log.clone(), &dest, delta.partition_num).context(format!(
            "Failed to get partition {} of device {:?}",
            delta.partition_num, dest
        ))?;

    info!(log, "Expanding filesystem in {:?}", partition_dev);
    expand_filesystem(&partition_dev, opts.tmp_mounts_dir)
        .context("Failed to expand filesystem after expanding partition")?;

    Ok(())
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

        let mut part = lo.to_string();
        part.push_str("p3");
        assert_eq!(
            get_partition_device(log, Path::new(&lo), 3)
                .context("Failed to get partition device")?,
            PathBuf::from(&part)
        );

        Ok(())
    }
}
