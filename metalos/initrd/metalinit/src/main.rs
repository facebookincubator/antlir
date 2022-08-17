use std::path::Path;
use std::path::PathBuf;
use std::thread;
use std::time;

use anyhow::bail;
use anyhow::Context;
use anyhow::Result;
use btrfs::SnapshotFlags;
use evalctx::Generator;
use evalctx::StarlarkGenerator;
use futures_util::FutureExt;
use kernel_cmdline::GenericCmdlineOpt;
use kernel_cmdline::KernelCmdArgs;
use kernel_cmdline::KnownArgs;
use lifecycle::stage;
use metalos_host_configs::host::HostConfig;
use metalos_mount::Mounter;
use metalos_mount::RealMounter;
use net_utils::get_mac;
use netlink::NlRoutingSocket;
use netlink::RtnlCachedLink;
use netlink::RtnlCachedLinkTrait;
use netlink::RtnlLinkCache;
use nix::mount::MsFlags;
use package_download::PackageExt;
use send_events::Event;
use send_events::EventSender;
use send_events::HttpSink;
use send_events::Source;
use slog::error;
use slog::info;
use slog::o;
use slog::Logger;
use state::State;
use structopt::clap::AppSettings;
use structopt::StructOpt;
use strum_macros::EnumIter;
use systemd::ActiveState;
use systemd::FilePath;
use systemd::LoadState;
use systemd::StartMode;
use systemd::Systemd;
use systemd::UnitName;
use tokio::try_join;

use crate::events::*;

mod events;

#[derive(StructOpt, Debug)]
struct Args {
    #[structopt(
        default_value = "usr/lib/metalos/generators",
        help = "Root of starlark generator files. If a relative path, it will \
        be interpreted as relative to --root."
    )]
    generators_root: PathBuf,

    #[structopt(long, default_value = "metalinit")]
    event_sender: String,
}

#[derive(EnumIter)]
enum MetalInitKnownArgs {
    Root,
    RootFsType,
    RootFlags,
    RootFlagRo,
    RootFlagRw,
    SendProvisioningEvents,
}

impl KnownArgs for MetalInitKnownArgs {
    fn flag_name(&self) -> &'static str {
        match self {
            Self::Root => "--root",
            Self::RootFsType => "--rootfstype",
            Self::RootFlags => "--rootflags",
            Self::RootFlagRo => "--ro",
            Self::RootFlagRw => "--rw",
            Self::SendProvisioningEvents => "--metalos.send_provisioning_events",
        }
    }
}

#[derive(Debug, StructOpt)]
#[structopt(name = "kernel-cmdline", setting(AppSettings::NoBinaryName))]
struct MetalInitArgs {
    #[structopt(parse(from_str = GenericCmdlineOpt::parse_arg))]
    #[allow(dead_code)]
    non_metalos_opts: Vec<GenericCmdlineOpt>,

    #[structopt(flatten)]
    mount_options: Root,

    #[structopt(long = &MetalInitKnownArgs::SendProvisioningEvents.flag_name())]
    send_provisioning_events: bool,
}

impl KernelCmdArgs for MetalInitArgs {
    type Args = MetalInitKnownArgs;
}

#[derive(Debug, StructOpt, PartialEq)]
struct Root {
    #[structopt(long = &MetalInitKnownArgs::Root.flag_name())]
    root: String,

    #[structopt(long = &MetalInitKnownArgs::RootFsType.flag_name(), default_value = "btrfs")]
    fstype: String,

    #[structopt(long = &MetalInitKnownArgs::RootFlags.flag_name())]
    flags: Option<Vec<String>>,

    #[structopt(long = &MetalInitKnownArgs::RootFlagRo.flag_name())]
    ro: bool,

    #[structopt(long = &MetalInitKnownArgs::RootFlagRw.flag_name())]
    rw: bool,
}

fn build_event_sender(config: &HostConfig, args: &Args) -> Result<EventSender<HttpSink>> {
    let sink = HttpSink::new(config.provisioning_config.event_backend.base_uri.clone());

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
    let kernel_args = MetalInitArgs::from_proc_cmdline().context("failed to parse kernel args")?;
    let boot_id = Bootloader::get_boot_id().context("Failed to get current boot_id")?;

    info!(
        log,
        "Running metalinit with args:\n{:?}\n\
        and kernel args:\n\
        {:?}\nfor boot: {:?}",
        args,
        kernel_args,
        boot_id
    );

    // Mount disk
    std::fs::create_dir_all(metalos_paths::control())
        .context(format!("failed to mkdir {:?}", metalos_paths::control()))?;
    Bootloader::mount_root(
        &RealMounter { log: log.clone() },
        &kernel_args.mount_options,
        metalos_paths::control(),
    )
    .context("Failed to mount root")?;

    // ensure the subvol hierarchy is correct
    metalos_paths_tmpfiles_integration::setup_tmpfiles()
        .context("while setting up subvol hierarchy")?;

    let config = HostConfig::current()
        .context("failed to load latest config from disk")?
        .context("No host config available")?;
    info!(log, "Found config on disk with value: {:?}", config);

    let events = build_event_sender(&config, &args).context("failed to build event sender")?;

    let mut bootloader = Bootloader {
        log: log.clone(),
        args,
        config,
        boot_id,
        mount_options: kernel_args.mount_options,
        events,
        send_events: kernel_args.send_provisioning_events,
    };

    bootloader.send_event(RamdiskReady {}).await;

    match bootloader.boot().await {
        Ok(()) => {
            error!(log, "unexpectedly returned from metalinit");
            bail!("unexpectedly returned from metalinit");
        }
        Err(err) => {
            error!(log, "failed to switch root: {:?}", err);
            bootloader.send_event(Failure { error: &err }).await;
            Err(err)
        }
    }
}

struct Bootloader {
    log: Logger,
    args: Args,
    config: HostConfig,
    boot_id: String,
    mount_options: Root,
    events: EventSender<HttpSink>,
    send_events: bool,
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

        if !self.send_events {
            return;
        }

        match self.events.send(event).await {
            Ok(_) => {}
            Err(err) => {
                error!(self.log, "failed to send event: {:?}", err);
            }
        }
    }

    async fn boot(&mut self) -> Result<()> {
        let dl =
            package_download::HttpsDownloader::new().context("while creating HttpsDownloader")?;
        // Download packages / stage
        try_join!(
            stage(
                self.log.clone(),
                dl.clone(),
                self.config.boot_config.clone()
            )
            .map(|r| r.context("while staging BootConfig")),
            stage(self.log.clone(), dl, self.config.runtime_config.clone())
                .map(|r| r.context("while staging RuntimeConfig")),
        )?;

        self.send_event(StagedConfigs {}).await;

        let root_subvol = self
            .config
            .boot_config
            .rootfs
            .on_disk()
            .context("rootfs not on disk")?;

        let kernel_subvol = self
            .config
            .boot_config
            .kernel
            .pkg
            .on_disk()
            .context("kernel subvol not on disk")?;

        // prepare new root
        let current_boot_dir =
            metalos_paths::runtime::boot().join(format!("{}:{}", 0, self.boot_id));
        let mut current_boot_subvol = root_subvol
            .snapshot(&current_boot_dir, SnapshotFlags::empty())
            .context(format!(
                "Failed to snapshot root from {:?} to {:?}",
                root_subvol.path(),
                current_boot_dir,
            ))?;
        current_boot_subvol
            .set_readonly(false)
            .context("Failed to set new boot subvol RW")?;

        // We mount a r/w snapshot of the kernel package so that we can upgrade kernel modules
        // during runtime. This is suboptimal, and we move towards making this mount immutable.
        // However, before we can do that, we likely need to change the way kernel modules are
        // packaged and deployed so we can ensure they never need to be upgraded outside the scope
        // of an offline-update. T126799147
        let current_kernel_dir =
            metalos_paths::runtime::kernel().join(format!("{}:{}", 0, self.boot_id));
        let current_kernel_subvol = kernel_subvol
            .snapshot(&current_kernel_dir, SnapshotFlags::empty())
            .context(format!(
                "Failed to snapshot kernel from {:?} to {:?}",
                kernel_subvol.path(),
                current_kernel_dir,
            ))?;

        // run generator

        // if --generators-root is absolute, this join will still do the right
        // thing, but otherwise makes it possible for users to pass a different
        // relative path if desired
        let generators_root = current_boot_subvol.path().join(&self.args.generators_root);

        let generators = StarlarkGenerator::load(&generators_root).context(format!(
            "failed to load generators from {:?}",
            &generators_root
        ))?;
        for gen in generators {
            let output = gen
                .eval(&self.config.provisioning_config)
                .context(format!("could not apply eval generator for {}", gen.name()))?;
            output.apply(self.log.clone(), current_boot_subvol.path())?;
        }

        // switch root
        self.switch_root(
            RealMounter {
                log: self.log.clone(),
            },
            current_boot_subvol.path(),
            current_kernel_subvol.path(),
        )
        .await
        .context("failed to switchroot into new boot snapshot")?;

        bail!("unexpectedly returned from switch_root");
    }

    async fn switch_root<M: Mounter>(
        &mut self,
        mounter: M,
        boot_snapshot: &Path,
        kernel_snapshot: &Path,
    ) -> Result<()> {
        let target_path = Path::new("/sysroot");
        std::fs::create_dir(&target_path).context(format!("failed to mkdir {:?}", target_path))?;

        let mut new_flags: Vec<String> = self
            .mount_options
            .flags
            .clone()
            .unwrap_or_default()
            .into_iter()
            .filter(|a| !a.starts_with("subvolid=") && !a.starts_with("subvol="))
            .collect();

        let new_subvol = boot_snapshot
            .strip_prefix(metalos_paths::control())
            .context(format!(
                "Provided snapshot {:?} didn't start with {:?}",
                boot_snapshot,
                metalos_paths::control(),
            ))?
            .to_str()
            .context(format!("snapshot {:?} was not valid utf-8", boot_snapshot))?
            .to_string();

        new_flags.push(format!("subvol=/volume/{}", new_subvol));
        self.mount_options.flags = Some(new_flags);

        Bootloader::mount_root(&mounter, &self.mount_options, target_path)
            .context(format!("Failed to remount onto {:?}", target_path))?;

        let utsname = nix::sys::utsname::uname();
        let mountpoint = target_path.join("usr/lib/modules").join(utsname.release());
        std::fs::create_dir_all(&mountpoint).context(format!(
            "while creating kernel modules mountpoint {}",
            mountpoint.display()
        ))?;

        let modules_dir = kernel_snapshot
            .join("modules")
            .to_str()
            .context("modules dir is not a string")?
            .to_string();

        mounter
            .mount(
                Path::new(&modules_dir),
                &mountpoint,
                Some("btrfs"),
                MsFlags::MS_BIND,
                None,
            )
            .context(format!(
                "while mounting kernel {} modules at {}",
                modules_dir,
                mountpoint.display()
            ))?;

        // Bind mount kernel-devel
        let mountpoint_build = target_path
            .join("lib/modules")
            .join(utsname.release())
            .join("build");
        std::fs::create_dir_all(&mountpoint_build).context(format!(
            "while creating kernel devel lib/modules/<>/build mountpoint {}",
            mountpoint_build.display()
        ))?;

        let mountpoint_source = target_path
            .join("lib/modules")
            .join(utsname.release())
            .join("source");
        std::fs::create_dir_all(&mountpoint_source).context(format!(
            "while creating kernel devel lib/modules/<>/source mountpoint {}",
            mountpoint_source.display()
        ))?;

        let modules_devel_dir = kernel_snapshot
            .join("devel")
            .to_str()
            .context("modules dir is not a string")?
            .to_string();
        // Ensure the kernel/devel directory exists.
        // If devel doesn't exist, downstream operations that depend on its contents will still break,
        // this is to unblock provisioning until we work out why some packages don't have this content.
        std::fs::create_dir_all(&modules_devel_dir).context(format!(
            "while creating devel dir in kernel snapshot at {}",
            modules_devel_dir
        ))?;

        mounter
            .mount(
                Path::new(&modules_devel_dir),
                &mountpoint_build,
                Some("btrfs"),
                MsFlags::MS_BIND,
                None,
            )
            .context(format!(
                "while mounting kernel {} modules at {}",
                modules_dir,
                mountpoint_build.display()
            ))?;

        mounter
            .mount(
                Path::new(&modules_devel_dir),
                &mountpoint_source,
                Some("btrfs"),
                MsFlags::MS_BIND,
                None,
            )
            .context(format!(
                "while mounting kernel {} modules at {}",
                modules_dir,
                mountpoint_source.display()
            ))?;

        let sd = Systemd::connect(self.log.clone()).await?;

        self.send_event(StartingSwitchroot { path: target_path })
            .await;

        info!(self.log, "Cleaning up networking");

        let five_seconds_ms = 5000;

        try_join!(
            self.stop_unit(&sd, "systemd-networkd.socket".to_string(), five_seconds_ms),
            self.stop_unit(&sd, "systemd-networkd.service".to_string(), five_seconds_ms),
        )
        .context("failed to stop network before switchroot")?;

        self.network_down()
            .context("failed to cleanup network links before switchroot")?;

        info!(self.log, "switch-rooting into {:?}", target_path);

        // ask systemd to switch-root to the new root fs
        sd.switch_root(FilePath::new(target_path), FilePath::new("/sbin/init"))
            .await
            .context(format!(
                "failed to trigger switch-root (systemctl switch-root {:?})",
                target_path
            ))
    }

    async fn stop_unit(&self, sd: &Systemd, name: String, timeout_ms: u64) -> Result<()> {
        let mut found = false;

        for u in sd
            .list_units()
            .await
            .context("failed to list systemd units")?
        {
            if name.eq(u.name.as_str()) {
                if u.load_state != LoadState::Loaded {
                    return Ok(());
                }
                if u.active_state == ActiveState::Active {
                    found = true;
                    sd.stop_unit(&UnitName::from(name.as_str()), &StartMode::Replace)
                        .await
                        .context(format!("failed to stop unit {}", &name))?;
                    // briefly sleep to give systemd a chance to take action
                    thread::sleep(time::Duration::from_millis(50));
                }

                break;
            }
        }

        if !found {
            return Ok(());
        }

        let deadline = time::Instant::now() + time::Duration::from_millis(timeout_ms);

        loop {
            found = false;

            for u in sd
                .list_units()
                .await
                .context("failed to list systemd units")?
            {
                if name.eq(u.name.as_str()) {
                    found = true;

                    if u.load_state != LoadState::Loaded || u.active_state == ActiveState::Inactive
                    {
                        return Ok(());
                    }

                    break;
                }
            }

            if !found {
                break;
            }

            if time::Instant::now() > deadline {
                bail!(
                    "failed to stop unit {} in {} milliseconds",
                    name,
                    timeout_ms
                );
            }

            thread::sleep(time::Duration::from_millis(100));
        }

        Ok(())
    }

    fn network_down(&self) -> Result<()> {
        info!(self.log, "Starting network_down()");

        let rsock = NlRoutingSocket::new()?;
        let rlc = RtnlLinkCache::new(&rsock)?;
        self.link_down::<RtnlCachedLink>(&rsock, rlc.links())?;

        info!(self.log, "Finished network_down()");
        Ok(())
    }

    fn link_down<T: RtnlCachedLinkTrait + std::fmt::Display>(
        &self,
        rsock: &NlRoutingSocket,
        rlc: &[T],
    ) -> Result<()> {
        for link in &mut rlc.iter() {
            info!(self.log, "Inspecting link: {}", link);

            // Look at all up links not named "lo".
            if !link.is_up()
                && link
                    .name()
                    .unwrap_or_else(|| "".to_string())
                    .starts_with("lo")
            {
                continue;
            }

            info!(self.log, "Downing link: {}", link);
            link.set_down(rsock)?;
        }
        Ok(())
    }

    fn get_boot_id() -> Result<String> {
        let content = std::fs::read_to_string(Path::new("/proc/sys/kernel/random/boot_id"))
            .context("Can't read /proc/sys/kernel/random/boot_id")?;

        Ok(content.trim().replace('-', ""))
    }

    fn mount_root<M: Mounter>(mounter: &M, root_args: &Root, target: &Path) -> Result<()> {
        let mut flags = MsFlags::empty();
        if root_args.ro && !root_args.rw {
            flags.insert(MsFlags::MS_RDONLY);
        }

        let source_device = blkid::evaluate_spec(&root_args.root)
            .context(format!("no device matches blkid spec '{}'", root_args.root))?;

        mounter
            .mount(
                &source_device,
                target,
                Some(&root_args.fstype),
                flags.clone(),
                root_args.flags.as_ref().map(|f| f.join(",")).as_deref(),
            )
            .context(format!(
                "failed to mount root partition {:?} to {:?} with flags {:?} {:?}",
                source_device, target, flags, root_args.flags,
            ))?;

        Ok(())
    }
}
