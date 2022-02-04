/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
#[cfg_attr(initrd, allow(unused_imports))]
use futures::future::try_join_all;
use slog::{o, trace, Logger};
use structopt::StructOpt;
use url::Url;

use btrfs::{SendstreamExt, Subvolume};
use image::download::{Downloader, HttpsDownloader};
use image::AnyImage;

use crate::load_host_config::get_host_config;

#[derive(StructOpt)]
pub struct Opts {
    host_config_uri: Url,
    basedir: PathBuf,
}

/// Download a single image into the given destination
async fn fetch_image(
    log: Logger,
    dl: impl Downloader,
    image: AnyImage,
    basedir: &Path,
) -> Result<Subvolume> {
    let dest = image.path_on_disk(basedir);
    let log = log.new(o!("package" => format!("{:?}", image), "dest" => format!("{:?}", dest)));
    if dest.exists() {
        trace!(log, "subvolume already exists, using pre-cached subvol")
    }

    std::fs::create_dir_all(dest.parent().context("cannot receive directly into /")?)
        .with_context(|| format!("while creating parent directory for {:?}", dest))?;

    let dst = Subvolume::create(&dest)
        .with_context(|| format!("while creating destination subvol {:?}", dest))?;
    trace!(log, "created destination subvolume");

    trace!(log, "opening sendstream https connection");
    let sendstream = dl
        .open_sendstream(&image)
        .await
        .with_context(|| format!("while starting sendstream for {:?}", image))?;
    trace!(log, "receiving sendstream");
    sendstream
        .receive_into(&dst)
        .await
        .context("while receiving")
}

/// Fetch all the immediately-necessary images from the host config. If in the
/// initrd, this is just the rootfs (and very soon the kernel).
pub async fn fetch_images(log: Logger, config: crate::Config, opts: Opts) -> Result<()> {
    let host = get_host_config(&opts.host_config_uri)
        .await
        .with_context(|| format!("while loading host config from {} ", opts.host_config_uri))?;

    // TODO: use fbpkg.proxy when in the rootfs
    let dl = HttpsDownloader::new(config.download.package_format_uri().to_string())
        .context("while creating downloader")?;

    #[cfg(initrd)]
    {
        let root_subvol = fetch_image(
            log,
            dl,
            host.runtime_config
                .rootfs
                .clone()
                .try_into()
                .with_context(|| {
                    format!(
                        "while converting rootfs image {:?}",
                        host.runtime_config.rootfs
                    )
                })?,
            &opts.basedir,
        )
        .await
        .context("while downloading the rootfs")?;

        // TODO: onboard this to generator_lib if there is more than one thing that
        // needs to be included here
        std::fs::write(
            "/run/metalos/image_paths_environment",
            format!("METALOS_OS_VOLUME={}\n", root_subvol.path().display()),
        )
        .context("while writing /run/metalos/image_paths_environment")?
    }
    #[cfg(not(initrd))]
    {
        try_join_all(host.runtime_config.manifest.images.iter().map(|i| {
            let log = log.clone();
            let dl = dl.clone();
            let basedir = &opts.basedir;
            async move {
                let image = i
                    .clone()
                    .try_into()
                    .with_context(|| format!("while converting {:?}", i))?;
                fetch_image(log, dl, image, basedir)
                    .await
                    .with_context(|| format!("while downloading {:?}", i))
            }
        }))
        .await?;
    }

    Ok(())
}
