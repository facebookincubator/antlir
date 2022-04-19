/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#![deny(warnings)]
// TODO(T113359879) this can be removed when there are no more separate builds
// of metalctl
#![allow(unused_crate_dependencies)]

#[cfg(test)]
#[macro_use]
extern crate metalos_macros;

use std::collections::VecDeque;
use std::fs;
use std::io::BufWriter;
use std::path::PathBuf;

use anyhow::{Context, Result};
use slog::{error, o, warn, Drain, Logger};
use slog_glog_fmt::kv_categorizer::ErrorCategorizer;
use structopt::clap::AppSettings;
use structopt::StructOpt;

#[cfg(initrd)]
mod apply_disk_image;
mod apply_host_config;
mod config;
mod fetch_images;
#[cfg(initrd)]
mod generator;
mod kernel_cmdline;
mod load_host_config;
mod mount;
#[cfg(initrd)]
mod network_cleanup;
mod send_event;
mod switch_root;
#[cfg(initrd)]
mod umount;
#[cfg(not(initrd))]
mod update;

pub use config::Config;

#[derive(StructOpt)]
enum Subcommand {
    /// Systemd unit generator
    #[cfg(initrd)]
    MetalosGenerator(generator::Opts),
    /// Mount a filesystem
    #[cfg(initrd)]
    Mount(mount::Opts),
    /// Unmount a filesystem
    #[cfg(initrd)]
    Umount(umount::Opts),
    /// Download images specified in the MetalOS host config
    FetchImages(fetch_images::Opts),
    /// Cleanup networking in preperation for switchroot
    #[cfg(initrd)]
    NetworkCleanup(network_cleanup::Opts),
    /// Setup the new rootfs and switch-root into it
    SwitchRoot(switch_root::Opts),
    /// Download a structured host config
    LoadHostConfig(load_host_config::Opts),
    /// Generate and apply a structured host config
    ApplyHostConfig(apply_host_config::Opts),
    /// Send an event to the event endpoint
    SendEvent(send_event::Opts),
    /// Apply a provided disk image to a specified disk and then
    /// upsize it to the maximum size
    #[cfg(initrd)]
    ApplyDiskImage(apply_disk_image::Opts),
    #[cfg(not(initrd))]
    #[structopt(flatten)]
    Update(update::Update),
}

#[derive(StructOpt)]
#[structopt(name = "metalctl", setting(AppSettings::NoBinaryName))]
struct MetalCtl {
    #[structopt(short, long, default_value("/etc/metalctl.toml"))]
    config: PathBuf,
    #[structopt(subcommand)]
    command: Subcommand,
}

fn setup_kmsg_logger(log: Logger) -> Result<Logger> {
    // metalos-generator has an additional logging drain setup that is not as
    // pretty looking as other slog drain formats, but is usable with /dev/kmsg.
    // Otherwise, the regular drain that logs to stderr silently disappears when
    // systemd runs the generator.
    let kmsg = fs::OpenOptions::new()
        .write(true)
        .open("/dev/kmsg")
        .context("failed to open /dev/kmsg for logging")?;
    let kmsg = BufWriter::new(kmsg);

    let decorator = slog_term::PlainDecorator::new(kmsg);
    let drain = slog_glog_fmt::GlogFormat::new(decorator, ErrorCategorizer).fuse();
    let drain = slog_async::Async::new(drain).build().fuse();

    Ok(slog::Logger::root(
        slog::Duplicate::new(log, drain).fuse(),
        o!(),
    ))
}

async fn run_command(mut args: VecDeque<std::ffi::OsString>, log: Logger) -> Result<()> {
    // Yeah, expect() is not the best thing to do, but really what else can we
    // do besides panic?
    let bin_path: PathBuf = args
        .pop_front()
        .context(format!("metalctl must have args[0] found: {:?}", args))?
        .into();

    let bin_name = bin_path.file_name().context(format!(
        "metalctl: argv[0] must be a file path found: {:?}",
        bin_path
    ))?;

    // If argv[0] is a symlink for a multicall utility, push the file name back
    // into the args array so that structopt will parse it correctly
    if !bin_name
        .to_str()
        .context(format!(
            "Failed to decode binary path \
             (path likely contains non-Unicode characters): \
             {}",
            bin_name.to_string_lossy()
        ))?
        .starts_with("metalctl")
    {
        args.push_front(bin_name.to_owned());
    }

    let options = MetalCtl::from_iter(args);

    let mut config: config::Config = match std::fs::read_to_string(&options.config) {
        Ok(config_str) => toml::from_str(&config_str).context("invalid config")?,
        Err(e) => {
            warn!(
                log,
                "failed to read config from {:?}, using defaults: {}", options.config, e
            );
            Default::default()
        }
    };
    config.apply_kernel_cmdline_overrides().unwrap();

    match options.command {
        #[cfg(initrd)]
        Subcommand::MetalosGenerator(opts) => generator::generator(log, opts),
        #[cfg(initrd)]
        Subcommand::Mount(opts) => mount::mount(log, opts),
        #[cfg(initrd)]
        Subcommand::Umount(opts) => umount::umount(opts),
        Subcommand::FetchImages(opts) => fetch_images::fetch_images(log, opts).await,
        #[cfg(initrd)]
        Subcommand::NetworkCleanup(opts) => network_cleanup::network_cleanup(log, opts),
        Subcommand::SwitchRoot(opts) => switch_root::switch_root(log, opts).await,
        Subcommand::ApplyHostConfig(opts) => apply_host_config::apply_host_config(log, opts).await,
        Subcommand::LoadHostConfig(opts) => load_host_config::load_host_config(opts).await,
        Subcommand::SendEvent(opts) => send_event::send_event(log, config, opts).await,
        #[cfg(initrd)]
        Subcommand::ApplyDiskImage(opts) => apply_disk_image::apply_disk_image(log, opts).await,
        #[cfg(not(initrd))]
        Subcommand::Update(update) => update.subcommand(log).await,
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let args: VecDeque<std::ffi::OsString> = std::env::args_os().collect();

    let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
    let log = if args
        .iter()
        .any(|a| a.to_string_lossy().contains("generator"))
    {
        match setup_kmsg_logger(log) {
            Ok(log) => log,
            Err(e) => {
                eprintln!("Failed to setup kmsg logger: {:?}", e);
                slog::Logger::root(slog_glog_fmt::default_drain(), o!())
            }
        }
    } else {
        log
    };

    match run_command(args, log.clone()).await {
        Ok(r) => Ok(r),
        Err(e) => {
            error!(log, "{}", e);
            Err(e)
        }
    }
}
