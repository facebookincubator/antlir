use std::path::{Path, PathBuf};
use std::{thread, time};

use anyhow::{bail, Context, Result};
use futures_util::FutureExt;
use nix::mount::MsFlags;
use slog::{info, o, Logger};
use structopt::clap::AppSettings;
use structopt::StructOpt;
use strum_macros::EnumIter;
use tokio::try_join;

use btrfs::SnapshotFlags;
use evalctx::{Generator, StarlarkGenerator};
use kernel_cmdline::{GenericCmdlineOpt, KernelCmdArgs, KnownArgs};
use lifecycle::stage;
use metalos_host_configs::host::HostConfig;
use metalos_mount::{Mounter, RealMounter};
use netlink::{NlRoutingSocket, RtnlCachedLink, RtnlCachedLinkTrait, RtnlLinkCache};
use package_download::PackageExt;
use state::State;
use systemd::{ActiveState, FilePath, LoadState, StartMode, Systemd, UnitName};

#[derive(StructOpt, Debug)]
struct Args {
    #[structopt(
        default_value = "usr/lib/metalos/generators",
        help = "Root of starlark generator files. If a relative path, it will \
        be interpreted as relative to --root."
    )]
    generators_root: PathBuf,
}

#[derive(EnumIter)]
enum MetalInitKnownArgs {
    Root,
    RootFsType,
    RootFlags,
    RootFlagRo,
    RootFlagRw,
}

impl KnownArgs for MetalInitKnownArgs {
    fn flag_name(&self) -> &'static str {
        match self {
            Self::Root => "--root",
            Self::RootFsType => "--rootfstype",
            Self::RootFlags => "--rootflags",
            Self::RootFlagRo => "--ro",
            Self::RootFlagRw => "--rw",
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

fn get_boot_id() -> Result<String> {
    let content = std::fs::read_to_string(Path::new("/proc/sys/kernel/random/boot_id"))
        .context("Can't read /proc/sys/kernel/random/boot_id")?;

    Ok(content.trim().replace('-', ""))
}

fn link_down<T: RtnlCachedLinkTrait + std::fmt::Display>(
    log: &Logger,
    rsock: &NlRoutingSocket,
    rlc: &[T],
) -> Result<()> {
    for link in &mut rlc.iter() {
        info!(log, "Inspecting link: {}", link);

        // Look at up links named "eth*".
        if !link.is_up()
            || !link
                .name()
                .unwrap_or_else(|| "".to_string())
                .starts_with("eth")
        {
            continue;
        }

        info!(log, "Downing link: {}", link);
        link.set_down(rsock)?;
    }
    Ok(())
}

fn network_down(log: Logger) -> Result<()> {
    info!(log, "Starting network_down()");

    let rsock = NlRoutingSocket::new()?;
    let rlc = RtnlLinkCache::new(&rsock)?;
    link_down::<RtnlCachedLink>(&log, &rsock, rlc.links())?;

    info!(log, "Finished network_down()");
    Ok(())
}

async fn stop_systemd_unit(sd: &Systemd, name: String, timeout_ms: u64) -> Result<()> {
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

                if u.load_state != LoadState::Loaded || u.active_state == ActiveState::Inactive {
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

async fn switch_root<M: Mounter>(
    log: Logger,
    mounter: M,
    mut mount_args: Root,
    snapshot: &Path,
    config: &HostConfig,
) -> Result<()> {
    let target_path = Path::new("/sysroot");
    std::fs::create_dir(&target_path).context(format!("failed to mkdir {:?}", target_path))?;

    let mut new_flags: Vec<String> = mount_args
        .flags
        .clone()
        .unwrap_or_default()
        .into_iter()
        .filter(|a| !a.starts_with("subvolid=") && !a.starts_with("subvol="))
        .collect();

    let new_subvol = snapshot
        .strip_prefix(metalos_paths::control())
        .context(format!(
            "Provided snapshot {:?} didn't start with {:?}",
            snapshot,
            metalos_paths::control(),
        ))?
        .to_str()
        .context(format!("snapshot {:?} was not valid utf-8", snapshot))?
        .to_string();

    new_flags.push(format!("subvol=/volume/{}", new_subvol));
    mount_args.flags = Some(new_flags);

    mount_root(&mounter, &mount_args, target_path)
        .context(format!("Failed to remount onto {:?}", target_path))?;

    let kernel_subvol = config
        .boot_config
        .kernel
        .pkg
        .on_disk()
        .context("kernel subvol not on disk")?;
    let utsname = nix::sys::utsname::uname();
    let mountpoint = target_path.join("usr/lib/modules").join(utsname.release());
    std::fs::create_dir_all(&mountpoint).context(format!(
        "while creating kernel modules mountpoint {}",
        mountpoint.display()
    ))?;

    let modules_dir = kernel_subvol
        .path()
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

    let sd = Systemd::connect(log.clone()).await?;

    info!(log, "Cleaning up networking");

    try_join!(
        stop_systemd_unit(&sd, "systemd-networkd.socket".to_string(), 5000),
        stop_systemd_unit(&sd, "systemd-networkd.service".to_string(), 5000),
    )
    .context("failed to stop network before switchroot")?;

    network_down(log.clone()).context("failed to cleanup network links before switchroot")?;

    info!(log, "switch-rooting into {:?}", target_path);

    // ask systemd to switch-root to the new root fs
    sd.switch_root(FilePath::new(target_path), FilePath::new("/sbin/init"))
        .await
        .context(format!(
            "failed to trigger switch-root (systemctl switch-root {:?})",
            target_path
        ))
}

#[tokio::main]
async fn main() -> Result<()> {
    let log = Logger::root(slog_glog_fmt::default_drain(), o!());
    let args = Args::from_args();
    let kernel_args = MetalInitArgs::from_proc_cmdline().context("failed to parse kernel args")?;
    let boot_id = get_boot_id().context("Failed to get current boot_id")?;

    info!(
        log,
        "Running metalinit with args:\n{:?}\n\
        and kernel args:\n \
        {:?}\nfor boot: {:?}",
        args,
        kernel_args,
        boot_id
    );

    // Mount disk
    std::fs::create_dir_all(metalos_paths::control())
        .context(format!("failed to mkdir {:?}", metalos_paths::control()))?;
    mount_root(
        &RealMounter { log: log.clone() },
        &kernel_args.mount_options,
        metalos_paths::control(),
    )
    .context("Failed to mount root")?;

    let config = HostConfig::current()
        .context("failed to load latest config from disk")?
        .context("No host config available")?;
    info!(log, "Found config on disk with value: {:?}", config);

    // Download packages / stage
    try_join!(
        stage(log.clone(), config.boot_config.clone())
            .map(|r| r.context("while staging BootConfig")),
        stage(log.clone(), config.runtime_config.clone())
            .map(|r| r.context("while staging RuntimeConfig")),
    )?;

    let root_subvol = config
        .boot_config
        .rootfs
        .on_disk()
        .context("rootfs not on disk")?;

    // prepare new root
    let current_boot_dir = metalos_paths::boots().join(format!("{}:{}", 0, boot_id));
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

    // run generator

    // if --generators-root is absolute, this join will still do the right
    // thing, but otherwise makes it possible for users to pass a different
    // relative path if desired
    let generators_root = current_boot_subvol.path().join(&args.generators_root);

    let generators = StarlarkGenerator::load(&generators_root).context(format!(
        "failed to load generators from {:?}",
        &generators_root
    ))?;
    for gen in generators {
        let output = gen
            .eval(&config.provisioning_config)
            .context(format!("could not apply eval generator for {}", gen.name()))?;
        output.apply(log.clone(), current_boot_subvol.path())?;
    }

    // switch root
    switch_root(
        log.clone(),
        RealMounter { log },
        kernel_args.mount_options,
        current_boot_subvol.path(),
        &config,
    )
    .await
    .context("failed to switchroot into new boot snapshot")?;

    Ok(())
}
