use std::path::PathBuf;

use anyhow::{Context, Result};
use slog::{info, Logger};
use structopt::StructOpt;
use url::Url;

use find_root_disk::{DiskPath, FindRootDisk, SingleDiskFinder};
use get_host_config::get_host_config;
use metalos_host_configs::packages::{Format, GptRootDisk};
use metalos_mount::RealMounter;

#[derive(StructOpt)]
pub struct Opts {
    host_config_uri: Url,

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

    let host = get_host_config(&opts.host_config_uri)
        .await
        .with_context(|| format!("while loading host config from {} ", opts.host_config_uri))?;

    ::apply_disk_image::apply_disk_image(
        log,
        disk,
        &host.provisioning_config.gpt_root_disk,
        &opts.tmp_mounts_dir,
        opts.buffer_size,
        RealMounter {},
    )
    .await?;

    Ok(())
}
