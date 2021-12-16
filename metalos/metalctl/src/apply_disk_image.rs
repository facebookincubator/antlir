use std::fs;
use std::io;
use std::io::{Read, Write};
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::process::Command;

use anyhow::{anyhow, Context, Result};
use slog::{info, Logger};
use structopt::StructOpt;

use expand_partition::expand_last_partition;

// define ioctl macros based on the codes in linux/fs.h
nix::ioctl_none!(ioctl_blkrrpart, 0x12, 95);

#[derive(StructOpt)]
pub struct Opts {
    source: PathBuf,
    dest: PathBuf,

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

pub async fn apply_disk_image(log: Logger, opts: Opts) -> Result<()> {
    let src_metadata = opts
        .source
        .metadata()
        .context("Could not get metadata for source file")?;
    let src_len = src_metadata.len();

    let mut src = fs::File::open(&opts.source).context("Failed to open source file")?;
    let mut dst = fs::OpenOptions::new()
        .write(true)
        .open(&opts.dest)
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
    info!(log, "Wrote {} bytes to {:?}", src_len, opts.dest);

    info!(log, "Expanding last partition of {:?}", opts.dest);
    let delta = expand_last_partition(&opts.dest).context(format!(
        "Failed to expand last partition of: {:?}",
        opts.dest
    ))?;
    info!(
        log,
        "Expanded {:?} partition {} from {} bytes to {} bytes with new last lba = {}",
        opts.dest,
        delta.partition_num,
        delta.old_size,
        delta.new_size,
        delta.new_last_lb,
    );

    if unsafe { libc::syncfs(dst.as_raw_fd()) } != 0 {
        return Err(nix::Error::last()).context("Failed to run syncfs");
    }

    info!(log, "Rescanning partition table for {:?}", opts.dest);
    rescan_partitions(&dst).context("Failed to rescan partitions after writing image")?;

    let mut partition_dev = opts.dest.into_os_string();
    partition_dev.push(delta.partition_num.to_string());
    let partition_dev = PathBuf::from(partition_dev);

    info!(log, "Expanding filesystem in {:?}", partition_dev);
    expand_filesystem(&partition_dev, opts.tmp_mounts_dir)
        .context("Failed to expand filesystem after expanding partition")?;

    Ok(())
}

// TODO(T108026401): We want to test this on it's own however we need multiple disks
// first in VMtest. I tried to test this with loopbacks, the rootfs and inside the
// initrd but each of those had their own blockers and were kind of hacks anyway.
// so for now the coverage comes from the end-to-end test (switch-root-reimage)
