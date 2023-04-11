/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::PathBuf;
use std::process::Command;
use std::process::Stdio;

use antlir2_isolate::isolate;
use antlir2_isolate::IsolationContext;
use antlir2_package_lib::run_cmd;
use antlir2_package_lib::Spec;
use anyhow::anyhow;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use btrfs_send_stream_upgrade_lib::upgrade::send_stream::SendStream;
use btrfs_send_stream_upgrade_lib::upgrade::send_stream_upgrade_options::SendStreamUpgradeOptions;
use clap::Parser;
use json_arg::JsonFile;
use tempfile::NamedTempFile;
use tracing::trace;
use tracing_subscriber::prelude::*;

#[derive(Parser, Debug)]
/// Package an image layer into a file
pub(crate) struct PackageArgs {
    #[clap(long)]
    /// Specifications for the packaging
    spec: JsonFile<Spec>,
    #[clap(long)]
    /// Path to output the image
    out: PathBuf,
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
        Spec::Btrfs {
            btrfs_packager_path,
            spec,
        } => {
            let btrfs_packager_path = btrfs_packager_path
                .into_iter()
                .next()
                .context("Expected exactly one arg to btrfs_packager_path")?;

            // The output path must exist before we can make an absolute path for it.
            let output_file = File::create(&args.out).context("failed to create output file")?;
            output_file
                .sync_all()
                .context("Failed to sync output file to disk")?;
            drop(output_file);

            // Write just our sub-spec for btrfs to a file for the packager
            let btrfs_spec_file =
                NamedTempFile::new().context("failed to create tempfile for spec json")?;

            serde_json::to_writer(btrfs_spec_file.as_file(), &spec)
                .context("failed to write json to tempfile")?;

            btrfs_spec_file
                .as_file()
                .sync_all()
                .context("failed to sync json tempfile content")?;

            let btrfs_spec_file_abs = btrfs_spec_file
                .path()
                .canonicalize()
                .context("Failed to build abs path for spec tempfile")?;

            let mut btrfs_package_cmd = Command::new("sudo");
            btrfs_package_cmd
                .arg("unshare")
                .arg("--mount")
                .arg("--pid")
                .arg("--fork")
                .arg(btrfs_packager_path)
                .arg("--spec")
                .arg(btrfs_spec_file_abs)
                .arg("--out")
                .arg(&args.out);

            let output = btrfs_package_cmd
                .output()
                .context("failed to spawn isolated btrfs-packager")?;

            println!(
                "btrfs-packager stdout:\n{}\nbtrfs-packager stderr\n{}",
                std::str::from_utf8(&output.stdout)
                    .context("failed to render btrfs-packager stdout")?,
                std::str::from_utf8(&output.stderr)
                    .context("failed to render btrfs-packager stderr")?,
            );

            match output.status.success() {
                true => Ok(()),
                false => Err(anyhow!(
                    "failed to run command {:?}: {:?}",
                    btrfs_package_cmd,
                    output
                )),
            }
        }
        Spec::SendstreamV2 {
            layer,
            compression_level,
        } => {
            let v1file = NamedTempFile::new()?;
            trace!("sending v1 sendstream to {}", v1file.path().display());
            ensure!(
                Command::new("sudo")
                    .arg("btrfs")
                    .arg("send")
                    .arg(&layer)
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
        Spec::SendstreamZst {
            layer,
            compression_level,
        } => {
            trace!("sending v1 sendstream to zstd");
            let mut btrfs_send = Command::new("sudo")
                .arg("btrfs")
                .arg("send")
                .arg(&layer)
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

        Spec::Vfat {
            build_appliance,
            layer,
            fat_size,
            label,
            size_mb,
        } => {
            let mut file = File::create(&args.out).context("failed to create output file")?;
            file.seek(SeekFrom::Start(size_mb * 1024 * 1024))
                .context("failed to seek output to specified size")?;
            file.write_all(&[0])
                .context("Failed to write dummy byte at end of file")?;
            file.sync_all()
                .context("Failed to sync output file to disk")?;
            drop(file);

            let input = layer
                .canonicalize()
                .context("failed to build abs path to layer")?;

            let output = args
                .out
                .canonicalize()
                .context("failed to build abs path to output")?;

            let isol_context = IsolationContext::builder(&build_appliance)
                .inputs(input.as_path())
                .outputs(output.as_path())
                .setenv(("RUST_LOG", std::env::var_os("RUST_LOG").unwrap_or_default()))
                .build();

            // Build the vfat disk file first
            let mut mkfs_iso = isolate(isol_context.clone()).into_command();
            let mkfs = mkfs_iso.arg("/usr/sbin/mkfs.vfat");
            if let Some(fat_size) = fat_size {
                mkfs.arg(format!("-F{}", fat_size));
            }
            if let Some(label) = label {
                mkfs.arg("-n").arg(label);
            }

            run_cmd(mkfs.arg(&output).stdout(Stdio::piped())).context("failed to mkfs.vfat")?;

            // mcopy all the files from the input layer directly into the vfat image.
            let paths = std::fs::read_dir(&input).context("Failed to list input directory")?;
            let mut sources = Vec::new();
            for path in paths {
                sources.push(path.context("failed to read next input path")?.path());
            }

            run_cmd(
                isolate(isol_context)
                    .into_command()
                    .arg("/usr/bin/mcopy")
                    .arg("-v")
                    .arg("-i")
                    .arg(&output)
                    .arg("-sp")
                    .args(sources)
                    .arg("::")
                    .stdout(Stdio::piped()),
            )
            .context("Failed to mcopy layer into new fs")?;

            Ok(())
        }

        Spec::CpioGZ {
            build_appliance,
            layer,
            compression_level,
        } => {
            File::create(&args.out).context("failed to create output file")?;

            let layer_abs_path = layer
                .canonicalize()
                .context("failed to build absolute path to layer")?;

            let output_abs_path = args
                .out
                .canonicalize()
                .context("failed to build abs path to output")?;

            let isol_context = IsolationContext::builder(&build_appliance)
                .inputs([layer_abs_path.as_path()])
                .outputs([output_abs_path.as_path()])
                .working_directory(std::env::current_dir().context("while getting cwd")?)
                .build();

            let cpio_script = format!(
                "set -ue -o pipefail; \
                /usr/bin/find . -mindepth 1 ! -type s | \
                LANG=C /usr/bin/sort | \
                LANG=C /usr/bin/cpio -o -H newc | \
                /usr/bin/gzip -{} --stdout > {}",
                compression_level,
                output_abs_path.as_path().display()
            );

            run_cmd(
                isolate(isol_context)
                    .into_command()
                    .arg("/bin/bash")
                    .arg("-c")
                    .arg(cpio_script)
                    .stdout(Stdio::piped()),
            )
            .context("Failed to build cpio archive")?;

            Ok(())
        }

        Spec::CpioZst {
            build_appliance,
            layer,
            compression_level,
        } => {
            File::create(&args.out).context("failed to create output file")?;

            let layer_abs_path = layer
                .canonicalize()
                .context("failed to build absolute path to layer")?;

            let output_abs_path = args
                .out
                .canonicalize()
                .context("failed to build abs path to output")?;

            let isol_context = IsolationContext::builder(&build_appliance)
                .inputs([layer_abs_path.as_path()])
                .outputs([output_abs_path.as_path()])
                .working_directory(std::env::current_dir().context("while getting cwd")?)
                .build();

            let cpio_script = format!(
                "set -ue -o pipefail; \
                /usr/bin/find . -mindepth 1 ! -type s | \
                LANG=C /usr/bin/sort | \
                LANG=C /usr/bin/cpio -o -H newc | \
                /usr/bin/zstd --compress -{} -T0 -f -o {}",
                compression_level,
                output_abs_path.as_path().display()
            );

            run_cmd(
                isolate(isol_context)
                    .into_command()
                    .arg("/bin/bash")
                    .arg("-c")
                    .arg(cpio_script)
                    .stdout(Stdio::piped()),
            )
            .context("Failed to build cpio archive")?;

            Ok(())
        }
    }
}
