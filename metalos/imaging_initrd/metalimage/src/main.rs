use std::path::PathBuf;

use anyhow::{Context, Result};
use nix::mount::MsFlags;
use reqwest::Url;
use slog::{info, o, Logger};
use structopt::clap::AppSettings;
use structopt::StructOpt;
use strum_macros::EnumIter;

use apply_disk_image::apply_disk_image;
use find_root_disk::{DiskPath, FindRootDisk, SingleDiskFinder};
use get_host_config::get_host_config;
use image::download::HttpsDownloader;
use image::PackageExt;
use kernel_cmdline::{GenericCmdlineOpt, KernelCmdArgs, KnownArgs};
use metalos_host_configs::host::HostConfig;
use metalos_kexec::KexecInfo;
use metalos_mount::{Mounter, RealMounter};
use net_utils::get_mac;
use send_events::{EventSender, HttpSink, Source};
use state::State;

use crate::events::RamdiskReady;

mod events;

#[derive(StructOpt, Debug)]
struct Args {
    /// A temporary directory in which we can mount things in order to resize
    /// partitions.
    #[structopt(long, parse(from_os_str), default_value = "/tmp/expand_root_mnt")]
    tmp_mounts_dir: PathBuf,

    /// Size of the read/write buffer to use in bytes
    /// defaults to the same as real dd
    #[structopt(default_value = "512")]
    buffer_size: usize,

    #[structopt(long, default_value = "metalimage")]
    event_sender: String,
}

#[derive(EnumIter)]
enum MetalImageKnownArgs {
    HostConfigUri,
}

impl KnownArgs for MetalImageKnownArgs {
    fn flag_name(&self) -> &'static str {
        match self {
            Self::HostConfigUri => "--metalos.host_config_uri",
        }
    }
}

#[derive(Debug, StructOpt)]
#[structopt(name = "kernel-cmdline", setting(AppSettings::NoBinaryName))]
struct MetalImageArgs {
    #[structopt(parse(from_str = GenericCmdlineOpt::parse_arg))]
    #[allow(dead_code)]
    non_metalos_opts: Vec<GenericCmdlineOpt>,

    #[structopt(long = &MetalImageKnownArgs::HostConfigUri.flag_name())]
    host_config_uri: Url,
}

impl KernelCmdArgs for MetalImageArgs {
    type Args = MetalImageKnownArgs;
}

fn build_event_sender(config: &HostConfig, args: &Args) -> Result<EventSender<HttpSink>> {
    let sink = HttpSink::new(
        config
            .provisioning_config
            .event_backend_base_uri
            .parse()
            .context("Failed to parse event backend uri")?,
    );

    Ok(EventSender::new(
        Source::Mac(get_mac().context("Failed to find mac address")?),
        args.event_sender.clone(),
        sink,
    ))
}

#[tokio::main]
async fn main() -> Result<()> {
    let log = Logger::root(slog_glog_fmt::default_drain(), o!());
    let args = Args::from_args();
    let kernel_args = MetalImageArgs::from_proc_cmdline().context("failed to parse kernel args")?;

    info!(
        log,
        "Running metalimage with args:\n{:?}\nand kernel args:\n {:?}", args, kernel_args
    );
    // get config
    let config = get_host_config(&kernel_args.host_config_uri)
        .await
        .context("failed to load the host config")?;

    info!(log, "Got config: {:?}", config);

    let event_sender =
        build_event_sender(&config, &args).context("failed to build event sender")?;

    event_sender
        .send(RamdiskReady {})
        .await
        .context("failed to send event")?;

    info!(log, "Sent ramdisk ready event");

    // select root disk
    let disk_finder = SingleDiskFinder::new();
    let disk = disk_finder
        .get_root_device()
        .context("Failed to find root device to write root_disk_package to")?
        .dev_node()
        .context("Failed to get the devnode for root disk")?;

    info!(log, "Found root disk {:?}", disk);

    let summary = apply_disk_image(
        log.clone(),
        disk,
        &config.provisioning_config.gpt_root_disk,
        &args.tmp_mounts_dir,
        args.buffer_size,
        RealMounter {},
    )
    .await
    .context("Failed to apply disk image")?;

    info!(
        log,
        "Applied disk image to {:?}. Summary:\n{:?}",
        summary.partition_device,
        summary.partition_delta
    );

    if !metalos_paths::control().exists() {
        std::fs::create_dir_all(metalos_paths::control())
            .context("failed to create control mount directory")?;
    }

    RealMounter {}
        .mount(
            &summary.partition_device,
            metalos_paths::control(),
            Some("btrfs"),
            MsFlags::empty(),
            None,
        )
        .context(format!(
            "failed to mount root partition {:?} to {:?}",
            summary.partition_device,
            metalos_paths::control(),
        ))?;

    info!(
        log,
        "Mounted rootfs {:?} to {:?}",
        &summary.partition_device,
        metalos_paths::control()
    );

    // Write config
    let token = config.save().context("failed to save config to disk")?;
    token.commit().context("Failed to commit config")?;

    info!(log, "Wrote config to disk");

    // Download the next stage initrd
    let downloader = HttpsDownloader::new().context("while creating downloader")?;

    let initrd_path = config
        .boot_config
        .initrd
        .download(log.clone(), &downloader)
        .await
        .context("failed to download next stage initrd")?;

    info!(log, "Downloaded initrd to: {:?}", initrd_path);

    let kernel_path = config
        .boot_config
        .kernel
        .pkg
        .download(log.clone(), &downloader)
        .await
        .context("failed to download kernel")?;

    info!(log, "Downloaded kernel to: {:?}", kernel_path);

    KexecInfo::try_from(&config)
        .context("Failed to build kexec info")?
        .kexec(log)
        .await
        .context("failed to perform kexec")?;

    Ok(())
}
