/*
 * Copyright (c) Facebook, Inc. and its affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs;
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{Context, Result};
use slog::{debug, info, o, Logger};
use structopt::StructOpt;

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

use crate::http::drain_stream;

pub async fn fetch_image(log: Logger, config: crate::Config, opts: Opts) -> Result<()> {
    let log = log.new(o!("package" => opts.package.clone(), "dest" => format!("{:?}", opts.dest)));
    fs::create_dir_all(&opts.dest)
        .with_context(|| format!("failed to create destination dir {:?}", opts.dest))?;

    let uri = config.download.package_uri(opts.package)?;
    debug!(log, "downloading from {}", uri);

    let body = crate::http::download_file(log.clone(), uri).await?;

    if opts.download_only {
        debug!(log, "downloading image as file");
        let dst = fs::File::create(opts.dest.join(&opts.download_filename))?;
        let mut dst = BufWriter::new(dst);
        match opts.decompress_download {
            true => {
                let mut decoder = zstd::stream::write::Decoder::new(dst)
                    .context("failed to initialize decompressor")?;
                drain_stream(body, &mut decoder).await?;
                decoder.flush()?;
            }
            false => {
                drain_stream(body, &mut dst).await?;
            }
        };
    } else {
        info!(log, "receiving image as a zstd-compressed sendstream");
        let mut child = Command::new("/sbin/btrfs")
            .args(&[&"receive".into(), &opts.dest])
            .stdin(Stdio::piped())
            .spawn()
            .context("btrfs receive command failed to start")?;
        let stdin = child.stdin.take().unwrap();
        let mut decoder = zstd::stream::write::Decoder::new(BufWriter::new(stdin))
            .context("failed to initialize decompressor")?;
        drain_stream(body, &mut decoder).await?;
        decoder.flush()?;
    }
    Ok(())
}
