/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use btrfs_send_stream_upgrade_lib::upgrade::send_stream::SendStream;
use btrfs_send_stream_upgrade_lib::upgrade::send_stream_upgrade_options::SendStreamUpgradeOptions;
use clap::Parser;
use json_arg::JsonFile;
use serde::Deserialize;
use tempfile::NamedTempFile;
use tracing::trace;
use tracing_subscriber::prelude::*;

#[derive(Parser, Debug)]
/// Package an image layer into a file
pub(crate) struct PackageArgs {
    #[clap(long)]
    /// Path to image layer
    layer: PathBuf,
    #[clap(long)]
    /// Specifications for the packaging
    spec: JsonFile<Spec>,
    #[clap(long)]
    /// Path to output the image
    out: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Spec {
    #[serde(rename = "sendstream.v2")]
    SendstreamV2 { compression_level: i32 },
    #[serde(rename = "sendstream.zst")]
    SendstreamZst { compression_level: i32 },
}

fn main() -> Result<()> {
    let args = PackageArgs::parse();

    tracing_subscriber::registry()
        .with(
            tracing_subscriber::fmt::Layer::default()
                .event_format(
                    tracing_glog::Glog::default()
                        .with_span_context(true)
                        .with_timer(tracing_glog::LocalTime::default()),
                )
                .fmt_fields(tracing_glog::GlogFields::default()),
        )
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    match args.spec.into_inner() {
        Spec::SendstreamV2 { compression_level } => {
            let v1file = NamedTempFile::new()?;
            trace!("sending v1 sendstream to {}", v1file.path().display());
            ensure!(
                Command::new("sudo")
                    .arg("btrfs")
                    .arg("send")
                    .arg(&args.layer)
                    .arg("-f")
                    .arg(v1file.path())
                    .spawn()?
                    .wait()?
                    .success(),
                "btrfs-send failed"
            );
            trace!("upgrading to v2 sendstream");
            let mut stream = SendStream::new(SendStreamUpgradeOptions {
                input: Some(v1file.path().to_path_buf()),
                output: Some(args.out),
                compression_level,
                ..Default::default()
            })
            .context("while creating sendstream upgrader")?;
            stream.upgrade().context("while upgrading sendstream")
        }
        Spec::SendstreamZst { compression_level } => {
            trace!("sending v1 sendstream to zstd");
            let mut btrfs_send = Command::new("sudo")
                .arg("btrfs")
                .arg("send")
                .arg(&args.layer)
                .stdout(Stdio::piped())
                .spawn()?;
            let mut zstd = Command::new("zstd")
                .arg("--compress")
                .arg(format!("-{compression_level}"))
                .arg("-o")
                .arg(args.out)
                .stdin(btrfs_send.stdout.take().expect("is a pipe"))
                .spawn()?;
            ensure!(zstd.wait()?.success(), "zstd failed");
            ensure!(btrfs_send.wait()?.success(), "btrfs-send failed");
            Ok(())
        }
    }
}
