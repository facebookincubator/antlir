use std::path::PathBuf;

use anyhow::{Context, Result};
use slog::{info, Logger};
use structopt::StructOpt;

use find_root_disk::{DiskPath, FindRootDisk, SingleDiskFinder};
use metalos_host_configs::packages::{Format, GptRootDisk};
use metalos_mount::RealMounter;

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

pub async fn cmd_apply_disk_image(log: Logger, opts: Opts) -> Result<()> {
    let disk_finder = SingleDiskFinder::new();
    let disk = disk_finder
        .get_root_device()
        .context("Failed to find root device to write root_disk_package to")?
        .dev_node()
        .context("Failed to get the devnode for root disk")?;

    info!(log, "Selected {:?} as root disk", disk);

    // TODO: make this an image::Image all the way through (add it to HostConfig)
    let (name, uuid) = opts
        .package
        .split_once(':')
        .context("package must have ':' separator")?;

    let package = GptRootDisk::new(
        name.into(),
        uuid.parse()
            .with_context(|| format!("{} is not a uuid", uuid))?,
        None,
        Format::File,
    );

    ::apply_disk_image::apply_disk_image(
        log,
        disk,
        &package,
        &opts.tmp_mounts_dir,
        opts.buffer_size,
        RealMounter {},
    )
    .await?;

    Ok(())
}
