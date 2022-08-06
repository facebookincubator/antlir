/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;
use anyhow::Result;
use metalos_host_configs::boot_config::BootConfig;
use metalos_host_configs::host::HostConfig;
use metalos_kexec::KexecInfo;
use nix::mount::mount;
use nix::mount::MsFlags;
use slog::crit;
use slog::info;
use slog::o;
use slog::Logger;
use state::State;

fn mount_control(log: Logger) -> Result<()> {
    std::fs::create_dir_all(metalos_paths::control())
        .context("while creating mountpoint for control fs")?;
    let device = blkid::evaluate_spec("LABEL=/").context("no device found for LABEL=/")?;
    info!(
        log,
        "mounting {} at {}",
        device.display(),
        metalos_paths::control().display()
    );
    mount::<_, _, _, str>(
        Some(&device),
        metalos_paths::control(),
        Some("btrfs"),
        MsFlags::empty(),
        None,
    )
    .with_context(|| {
        format!(
            "while mounting {} at {}",
            device.display(),
            metalos_paths::control().display()
        )
    })
}

async fn real_main(log: Logger) -> Result<()> {
    mount_control(log.clone())?;

    let boot_config = BootConfig::staged()
        .context("while loading staged BootConfig")?
        .context("no BootConfig is staged")?;
    let log = log.new(o!("boot_config" => format!("{:?}", boot_config)));
    // mark boot config as actually committed now, so `BootConfig::current()`
    // from the rootfs will only ever show the actual current bootconfig, not
    // what will be applied at the next boot
    boot_config
        .save()
        .context("while re-saving staged BootConfig")?
        .commit()
        .context("while committing BootConfig")?;
    info!(log, "marked BootConfig as committed");

    // merge that BootConfig with the full HostConfig
    let mut host_config = HostConfig::current()
        .context("while loading committed HostConfig")?
        .context("no HostConfig is committed")?;
    host_config.boot_config = boot_config;
    let host_config_token = host_config
        .save()
        .context("while saving merged HostConfig")?;
    host_config_token
        .commit()
        .context("while committing merged HostConfig")?;
    info!(log, "marked merged HostConfig as committed");

    let kexec_info = KexecInfo::new_from_packages(
        &host_config.boot_config.kernel,
        &host_config.boot_config.initrd,
        format!(
            "{} metalos.bootloader=1",
            host_config.boot_config.kernel.cmdline,
        ),
    )
    .context("while building KexecInfo")?;

    kexec_info
        .kexec(log)
        .await
        .context("while invoking kexec")?;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<()> {
    let log = slog::Logger::root(slog_glog_fmt::default_drain(), o!());
    if let Err(e) = real_main(log.clone()).await {
        crit!(log, "{}", e);
        Err(e)
    } else {
        Ok(())
    }
}
