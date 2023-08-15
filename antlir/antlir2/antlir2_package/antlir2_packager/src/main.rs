/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Write as _;
use std::fs::File;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write as _;
use std::path::Path;
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
use anyhow::Error;
use anyhow::Result;
use clap::Parser;
use itertools::Itertools;
use json_arg::JsonFile;
use retry::delay::Fixed;
use retry::retry;
use tempfile::NamedTempFile;
use tempfile::PersistError;
use tracing::trace;
use tracing::warn;
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
        // Uncompressed sendstream, for Antlir1 compat
        Spec::Sendstream { layer } => {
            let v1file = retry(Fixed::from_millis(10_000).take(10), || {
                let v1file = NamedTempFile::new()?;
                trace!("sending v1 sendstream to {}", v1file.path().display());
                if Command::new("sudo")
                    .arg("btrfs")
                    .arg("send")
                    .arg(&layer)
                    .stdout(
                        v1file
                            .as_file()
                            .try_clone()
                            .context("while cloning v1file")?,
                    )
                    .spawn()
                    .context("while spawning btrfs-send")?
                    .wait()
                    .context("while waiting for btrfs-send")?
                    .success()
                {
                    Ok(v1file)
                } else {
                    Err(anyhow!("btrfs-send failed"))
                }
            })
            .map_err(Error::msg)
            .context("btrfs-send failed too many times")?;
            if let Err(PersistError { file, error }) = v1file.persist(&args.out) {
                warn!("failed to persist tempfile, falling back on full copy: {error:?}");
                std::fs::copy(file.path(), &args.out)
                    .context("while copying sendstream to output")?;
            }
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
            let mut mkfs = isolate(isol_context.clone())?.command("/usr/sbin/mkfs.vfat")?;
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
                isolate(isol_context)?
                    .command("/usr/bin/mcopy")?
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

        Spec::Cpio {
            build_appliance,
            layer,
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
                pushd '{}'; \
                /usr/bin/find . -mindepth 1 ! -type s | \
                LANG=C /usr/bin/sort | \
                LANG=C /usr/bin/cpio -o -H newc \
                > {}",
                layer_abs_path.display(),
                output_abs_path.as_path().display()
            );

            run_cmd(
                isolate(isol_context)?
                    .command("/bin/bash")?
                    .arg("-c")
                    .arg(cpio_script)
                    .stdout(Stdio::piped()),
            )
            .context("Failed to build cpio archive")?;

            Ok(())
        }

        Spec::Rpm {
            build_appliance,
            layer,
            name,
            epoch,
            version,
            release,
            arch,
            summary,
            license,
            requires,
            recommends,
            provides,
            supplements,
            conflicts,
            description,
            post_install_script,
        } => {
            let layer_abspath = layer
                .canonicalize()
                .context("failed to build absolute path to layer")?;

            let requires = requires
                .into_iter()
                .map(|r| format!("Requires: {r}"))
                .join("\n");

            let recommends = recommends
                .into_iter()
                .map(|r| format!("Recommends: {r}"))
                .join("\n");

            let provides = provides
                .into_iter()
                .map(|p| format!("Provides: {p}"))
                .join("\n");

            let supplements = supplements
                .into_iter()
                .map(|p| format!("Supplements: {p}"))
                .join("\n");

            let conflicts = conflicts
                .into_iter()
                .map(|p| format!("Conflicts: {p}"))
                .join("\n");

            let mut spec = format!(
                r#"Name: {name}
Epoch: {epoch}
Version: {version}
Release: {release}
BuildArch: {arch}

Summary: {summary}
License: {license}

{requires}
{recommends}
{provides}
{supplements}
{conflicts}

%description
{description}

{post_install_script}

"#,
                summary = summary.as_deref().unwrap_or(name.as_str()),
                requires = requires,
                recommends = recommends,
                provides = provides,
                supplements = supplements,
                conflicts = conflicts,
                description = description.as_deref().unwrap_or_default(),
                post_install_script = post_install_script
                    .map(|s| format!("%post\n{s}\n"))
                    .unwrap_or_default(),
            );
            if std::fs::read_dir(&layer)
                .context("failed to list layer contents")?
                .count()
                != 0
            {
                spec.push_str("%install\n");
                writeln!(
                    spec,
                    "cp -rp \"{layer}\"/* %{{buildroot}}/",
                    layer = layer_abspath.display()
                )?;
                spec.push_str("%files\n");
                for entry in walkdir::WalkDir::new(&layer) {
                    let entry = entry.context("while walking layer")?;
                    let relpath = Path::new("/").join(
                        entry
                            .path()
                            .strip_prefix(&layer)
                            .expect("must be under layer"),
                    );
                    if relpath == Path::new("/") {
                        continue;
                    }
                    spec.push_str(relpath.to_str().expect("our paths are always valid utf8"));
                    spec.push('\n');
                }
            } else {
                spec.push_str("%files\n");
            }
            let mut rpm_spec_file =
                NamedTempFile::new().context("failed to create tempfile for rpm spec")?;
            rpm_spec_file
                .write(spec.as_bytes())
                .context("while writing rpm spec file")?;

            let output_dir =
                tempfile::tempdir().context("while creating temp dir for rpm output")?;

            // create the arch-specific output dir explicitly so that it'll be
            // owned by the build user on the host, not root
            std::fs::create_dir(output_dir.path().join(&arch))
                .context("while creating output dir")?;

            let isol_context = IsolationContext::builder(&build_appliance)
                .inputs([rpm_spec_file.path(), layer.as_path()])
                .outputs([output_dir.path()])
                .working_directory(std::env::current_dir().context("while getting cwd")?)
                .build();

            run_cmd(
                isolate(isol_context)?
                    .command("/bin/rpmbuild")?
                    .arg("-bb")
                    .arg("--define")
                    .arg(format!("_rpmdir {}", output_dir.path().display()))
                    .arg(rpm_spec_file.path())
                    .stdout(Stdio::piped()),
            )
            .context("Failed to build rpm")?;

            let outputs: Vec<_> = output_dir
                .path()
                .join(arch)
                .read_dir()
                .context("while reading rpm output dir")?
                .filter_map(Result::ok)
                .collect();

            ensure!(
                outputs.len() == 1,
                "expected exactly one output rpm file, got: {outputs:?}"
            );

            std::fs::copy(outputs[0].path(), args.out)
                .context("while moving output to correct location")?;

            // fail loudly if there was a permissions error removing the
            // temporary output directory, otherwise a later buck build will
            // fail with permissions errors - spooky action at a distance
            output_dir
                .close()
                .context("while cleaning up output tmpdir")?;

            Ok(())
        }

        Spec::SquashFs {
            build_appliance,
            layer,
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

            let squashfs_script = format!(
                "set -ue -o pipefail; \
                /usr/sbin/mksquashfs {} {} -comp zstd -noappend -one-file-system",
                layer_abs_path.as_path().display(),
                output_abs_path.as_path().display()
            );

            run_cmd(
                isolate(isol_context)?
                    .command("/bin/bash")?
                    .arg("-c")
                    .arg(squashfs_script)
                    .stdout(Stdio::piped()),
            )
            .context("Failed to build cpio archive")?;

            Ok(())
        }

        Spec::Tar {
            build_appliance,
            layer,
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

            let tar_script = format!(
                "tar -c \
                --sparse \
                --one-file-system \
                --acls \
                --xattrs \
                --to-stdout \
                -C \
                {} \
                . \
                > {}",
                layer_abs_path.display(),
                output_abs_path.as_path().display(),
            );

            run_cmd(
                isolate(isol_context)?
                    .command("/bin/bash")?
                    .arg("-c")
                    .arg(tar_script)
                    .stdout(Stdio::piped()),
            )
            .context("Failed to build tar")?;

            Ok(())
        }
    }
}
