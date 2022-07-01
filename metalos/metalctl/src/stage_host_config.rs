/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use futures::try_join;
use futures::FutureExt;
use slog::Logger;
use url::Url;

use lifecycle::stage;
use package_download::PackageExt;

use get_host_config::get_host_config;

#[derive(Parser)]
pub struct Opts {
    host_config_uri: Url,
}

/// Fetch all the images from the host config.
pub async fn stage_host_config(log: Logger, opts: Opts) -> Result<()> {
    let host = get_host_config(&opts.host_config_uri)
        .await
        .with_context(|| format!("while loading host config from {} ", opts.host_config_uri))?;

    try_join!(
        stage(log.clone(), host.boot_config.clone()).map(|r| r.context("while staging BootConfig")),
        stage(log.clone(), host.runtime_config).map(|r| r.context("while staging RuntimeConfig")),
    )?;

    let root_subvol = host
        .boot_config
        .rootfs
        .on_disk()
        .context("rootfs not on disk")?;

    // TODO: onboard this to systemd_generator_lib if there is a lot more that
    // needs to be included here
    std::fs::write(
        "/run/metalos/image_paths_environment",
        format!("METALOS_OS_VOLUME={}\n", root_subvol.path().display()),
    )
    .context("while writing /run/metalos/image_paths_environment")?;

    Ok(())
}
