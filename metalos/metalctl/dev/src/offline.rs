/*
 * Copyright (c) Meta Platforms, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */
use std::ffi::CString;
use std::fs::File;
use std::io::{Seek, Write};
use std::os::unix::io::FromRawFd;
use std::os::unix::process::CommandExt;

use anyhow::{Context, Result};
use clap::Parser;
use slog::{debug, Logger};

use metalos_host_configs::api::OfflineUpdateRequest;
use metalos_host_configs::host::HostConfig;
use state::State;

use crate::{FilePackage, PackageArg, SendstreamPackage};

#[derive(Parser)]
pub(crate) struct Opts {
    #[clap(long)]
    rootfs: Option<PackageArg<SendstreamPackage>>,
    #[clap(long)]
    kernel: Option<PackageArg<SendstreamPackage>>,
    #[clap(long)]
    initrd: Option<PackageArg<FilePackage>>,
    #[clap(long, help = "defaults to current cmdline")]
    kernel_cmdline: Option<String>,
}

pub(crate) async fn offline(log: Logger, opts: Opts) -> Result<()> {
    let mut boot_config = HostConfig::current()
        .context("while loading current HostConfig")?
        .context("no committed HostConfig")?
        .boot_config;

    if let Some(rootfs) = opts.rootfs {
        boot_config.rootfs = rootfs.into();
    }
    if let Some(kernel) = opts.kernel {
        boot_config.kernel.pkg = kernel.into();
    }
    if let Some(kernel_cmdline) = opts.kernel_cmdline {
        boot_config.kernel.cmdline = kernel_cmdline;
    } else {
        let current_cmdline = std::fs::read_to_string("/proc/cmdline")
            .context("while reading current kernel cmdline")?;
        debug!(log, "using kernel cmdline = '{}'", current_cmdline);
        boot_config.kernel.cmdline = current_cmdline;
    }
    if let Some(initrd) = opts.initrd {
        boot_config.initrd = initrd.into();
    }

    lifecycle::stage(log, boot_config.clone())
        .await
        .context("while staging BootConfig packages")?;

    let req = OfflineUpdateRequest { boot_config };
    let input = fbthrift::simplejson_protocol::serialize(&req);

    let mut stdin = unsafe {
        File::from_raw_fd(
            nix::sys::memfd::memfd_create(
                &CString::new("input")
                    .expect("creating cstr can never fail with this static input"),
                nix::sys::memfd::MemFdCreateFlag::empty(),
            )
            .context("while creating BootConfig memfd")?,
        )
    };
    stdin
        .write_all(&input)
        .context("while writing BootConfig to memfd")?;
    stdin.rewind().context("while rewinding BootConfig memfd")?;

    Err(std::process::Command::new("metalctl")
        .arg("offline-update")
        .arg("commit")
        .arg("-")
        .stdin(stdin)
        .exec())
    .context("while execing into 'metalctl offline-update commit'")
}
