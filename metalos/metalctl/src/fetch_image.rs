/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::{copy, Cursor};
use std::path::PathBuf;

use anyhow::{Context, Result};
use slog::{o, trace, Logger};
use structopt::StructOpt;

use btrfs::{SendstreamExt, Subvolume};
use image::download::{Downloader, HttpsDownloader};
use image::AnyImage;

#[derive(StructOpt)]
pub struct Opts {
    package: String,
    dest: PathBuf,
    #[structopt(long)]
    download_only: bool,
    #[structopt(long)]
    decompress_download: bool,
    #[structopt(long, default_value = "download")]
    download_filename: String,
}

pub async fn fetch_image(log: Logger, config: crate::Config, opts: Opts) -> Result<()> {
    let log = log.new(o!("package" => opts.package.clone(), "dest" => format!("{:?}", opts.dest)));
    // TODO(vmagro): make this an image::Image all the way through
    let (name, id) = opts
        .package
        .split_once(':')
        .context("package must have ':' separator")?;
    let image: AnyImage = package_manifest::types::Image {
        name: name.into(),
        id: id.into(),
        kind: package_manifest::types::Kind::ROOTFS,
    }
    .try_into()
    .context("converting image representation")?;

    std::fs::create_dir_all(
        opts.dest
            .parent()
            .context("cannot receive directly into /")?,
    )
    .with_context(|| format!("while creating parent directory for {:?}", opts.dest))?;

    let dl = HttpsDownloader::new(config.download.package_format_uri().to_string())
        .context("while creating downloader")?;

    if opts.download_only {
        let url = dl.image_url(&image).context("while getting image url")?;
        let client: reqwest::Client = dl.into();
        let bytes = client
            .get(url.clone())
            .send()
            .await
            .with_context(|| format!("while opening {}", url))?
            .bytes()
            .await
            .with_context(|| format!("while reading {}", url))?;
        std::fs::create_dir(&opts.dest).with_context(|| {
            format!(
                "while creating parent directory for download {:?}",
                opts.dest
            )
        })?;
        let outpath = opts.dest.join(opts.download_filename);
        let mut out = File::create(&outpath)
            .with_context(|| format!("while creating {}", outpath.display()))?;
        if opts.decompress_download {
            zstd::stream::copy_decode(&mut Cursor::new(&bytes), &mut out)
                .with_context(|| format!("while decompressing to {}", outpath.display()))?;
        } else {
            copy(&mut Cursor::new(&bytes), &mut out)
                .with_context(|| format!("while writing to {}", outpath.display()))?;
        }
        return Ok(());
    }

    let dst = Subvolume::create(&opts.dest)
        .with_context(|| format!("while creating destination subvol {:?}", opts.dest))?;
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
        .context("while receiving")?;
    Ok(())
}
