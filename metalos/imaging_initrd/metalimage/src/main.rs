use std::path::PathBuf;

use anyhow::{Context, Result};
use futures::{try_join, FutureExt};
use nix::mount::MsFlags;
use reqwest::Url;
use slog::{error, info, o, Logger};
use structopt::clap::AppSettings;
use structopt::StructOpt;
use strum_macros::EnumIter;

use apply_disk_image::apply_disk_image;
use disk_wipe::lazy_wipe;
use find_root_disk::{DiskPath, FindRootDisk, SingleDiskFinder};
use get_host_config::get_host_config;
use kernel_cmdline::{GenericCmdlineOpt, KernelCmdArgs, KnownArgs};
use metalos_host_configs::host::HostConfig;
use metalos_kexec::KexecInfo;
use metalos_mount::{Mounter, RealMounter};
use net_utils::get_mac;
use package_download::{ensure_package_on_disk, HttpsDownloader};
use send_events::{Event, EventSender, HttpSink, Source};
use state::State;

use crate::events::*;

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

    let config = get_host_config(&kernel_args.host_config_uri)
        .await
        .context("failed to load the host config")?;

    info!(log, "Got config: {:?}", config);

    let events = build_event_sender(&config, &args).context("failed to build event sender")?;

    events
        .send(RamdiskReady {})
        .await
        .context("failed to send event")?;
    info!(log, "Sent ramdisk ready event");

    let bootloader = Bootloader {
        log: log.clone(),
        args,
        config,
        events,
    };

    match bootloader.boot().await {
        Ok(()) => Ok(()),
        Err(err) => {
            error!(log, "failed to boot into next stage: {:?}", err);
            bootloader.send_event(Failure { error: &err }).await;
            Err(err)
        }
    }
}

struct Bootloader {
    log: Logger,
    args: Args,
    config: HostConfig,
    events: EventSender<HttpSink>,
}

impl Bootloader {
    async fn send_event<T, E>(&self, event: T)
    where
        T: TryInto<Event, Error = E>,
        E: std::error::Error,
    {
        let event = match event.try_into() {
            Ok(event) => event,
            Err(err) => {
                error!(self.log, "failed to convert event: {:?}", err);
                return;
            }
        };

        match self.events.send(event).await {
            Ok(_) => {}
            Err(err) => {
                error!(self.log, "failed to send event: {:?}", err);
            }
        }
    }

    async fn boot(&self) -> Result<()> {
        // Select root disk
        let disk_finder = SingleDiskFinder::new();
        let mut disk = disk_finder
            .get_root_device()
            .context("Failed to find root device to write root_disk_package to")?
            .dev_node()
            .context("Failed to get the devnode for root disk")?;

        self.send_event(FoundRootDisk { path: &disk }).await;
        info!(self.log, "Found root disk {:?}", disk);

        // Clear any existing partition information
        let rwdisk = disk
            .open_rw_file()
            .context("Failed to open root disk devnode")?;
        lazy_wipe(rwdisk).context("Failed to wipe the root disk")?;

        // Download and apply disk image
        let summary = apply_disk_image(
            self.log.clone(),
            disk,
            self.config.provisioning_config.gpt_root_disk.clone(),
            &self.args.tmp_mounts_dir,
            self.args.buffer_size,
            RealMounter {},
        )
        .await
        .context("Failed to apply disk image")?;

        self.send_event(AppliedDiskImage {
            package: &self.config.provisioning_config.gpt_root_disk,
        })
        .await;
        info!(
            self.log,
            "Applied disk image to {:?}. Summary:\n{:?}",
            summary.partition_device,
            summary.partition_delta
        );

        // Mount rootfs
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

        self.send_event(MountedRootfs {
            source: &summary.partition_device,
            target: metalos_paths::control(),
        })
        .await;
        info!(
            self.log,
            "Mounted rootfs {:?} to {:?}",
            &summary.partition_device,
            metalos_paths::control()
        );

        // Write config
        let token = self
            .config
            .save()
            .context("failed to save config to disk")?;
        token.commit().context("Failed to commit config")?;

        self.send_event(WrittenConfig {}).await;
        info!(self.log, "Wrote config to disk");

        // Download the next stage initrd
        let downloader = HttpsDownloader::new().context("while creating downloader")?;

        let (initrd, kernel) = try_join!(
            ensure_package_on_disk(
                self.log.clone(),
                &downloader,
                self.config.boot_config.initrd.clone()
            )
            .map(|r| r.context("failed to download next stage initrd")),
            ensure_package_on_disk(
                self.log.clone(),
                &downloader,
                self.config.boot_config.kernel.pkg.clone()
            )
            .map(|r| r.context("failed to download kernel")),
        )?;

        self.send_event(DownloadedNextStage {
            kernel_package: &self.config.boot_config.kernel.pkg,
            initrd_package: &self.config.boot_config.initrd,
        })
        .await;
        info!(self.log, "Downloaded initrd to: {:?}", initrd.display());
        info!(
            self.log,
            "Downloaded kernel to: {:?}",
            kernel.path().display()
        );

        // Try to kexec
        self.send_event(StartingKexec {
            cmdline: &self.config.boot_config.kernel.cmdline,
        })
        .await;
        info!(self.log, "Trying to kexec");
        KexecInfo::try_from(&self.config)
            .context("Failed to build kexec info")?
            .kexec(self.log.clone())
            .await
            .context("failed to perform kexec")?;

        Ok(())
    }
}
