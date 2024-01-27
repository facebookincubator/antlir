/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

use antlir2_isolate::nspawn;
use antlir2_isolate::IsolationContext;
use anyhow::ensure;
use anyhow::Context;
use anyhow::Error;
use anyhow::Result;
use bytesize::ByteSize;
use serde::Deserialize;
use walkdir::WalkDir;

use crate::run_cmd;
use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
pub struct Ext3 {
    build_appliance: PathBuf,
    layer: PathBuf,
    label: Option<String>,
    size_mb: Option<u64>,
    free_mb: u64,
}

impl PackageFormat for Ext3 {
    fn build(&self, out: &Path) -> Result<()> {
        let rootless = antlir2_rootless::init().context("while initializing rootless")?;
        File::create(out).context("failed to create output file")?;

        let layer_abs_path = self
            .layer
            .canonicalize()
            .context("failed to build absolute path to layer")?;

        let output_abs_path = out
            .canonicalize()
            .context("failed to build abs path to output")?;

        let isol_context = IsolationContext::builder(&self.build_appliance)
            .inputs([layer_abs_path.as_path()])
            .outputs([output_abs_path.as_path()])
            .working_directory(std::env::current_dir().context("while getting cwd")?)
            .build();

        let isol = nspawn(isol_context)?;
        let mut cmd = isol.command("mkfs.ext3")?;
        if let Some(label) = &self.label {
            cmd.arg("-L").arg(label);
        }
        cmd.arg("-d").arg(&layer_abs_path);
        cmd.arg(&output_abs_path);
        if let Some(size_mb) = self.size_mb {
            cmd.arg(format!("{}M", size_mb));
            rootless.as_root(|| {
                run_cmd(cmd.stdout(Stdio::piped())).context("failed to build ext3 archive")
            })??;
        } else {
            let total_file_size = ByteSize::b(rootless.as_root(|| {
                Ok::<_, Error>(
                    WalkDir::new(&layer_abs_path)
                        .into_iter()
                        .map(|entry| {
                            entry.context("while walking directory").and_then(|e| {
                                e.metadata().map(|m| m.len()).with_context(|| {
                                    format!("while getting size of {}", e.path().display())
                                })
                            })
                        })
                        .collect::<Result<Vec<_>>>()?
                        .into_iter()
                        .sum(),
                )
            })??);
            // Well this is kinda crazy... Here goes:
            // We can't really determine the minimal size of an ext3 image file
            // given a directory - we can only approximate it.
            // The "user annoyance factor" of a failed build is *extremely*
            // high, so let's dramatically overestimate (25% more than what we
            // think) the space that we might need, create an ext3 filesystem
            // with that much space, then shrink it down.
            let size = ByteSize::b((total_file_size.0 as f64 * 1.25) as u64);
            let size = std::cmp::max(
                size,
                // ext3 filesystems must be at least 2 MiB
                ByteSize::mib(2),
            );
            // Round up
            // It's just one kilobyte Michael, what could it cost? $10?
            let size_kb = (size.0 / 1024) + 1;
            cmd.arg(format!("{size_kb}K"));
            rootless.as_root(|| {
                run_cmd(cmd.stdout(Stdio::piped())).context("failed to build ext3 archive")
            })??;

            let out = rootless
                .as_root(|| run_cmd(isol.command("resize2fs")?.arg("-M").arg(&output_abs_path)))?
                .context("while minimizing fs size")?;
            ensure!(out.status.success(), "resize2fs failed");

            // Now, if the user asked for some free space, we need to give it to
            // them.
            if self.free_mb != 0 {
                let f = std::fs::OpenOptions::new()
                    .write(true)
                    .open(&output_abs_path)
                    .context("while opening image file")?;
                let size = f.metadata().context("while getting image size")?.len();

                let new_size = size + ByteSize::mib(self.free_mb);
                f.set_len(new_size.0).context("while growing image file")?;
                rootless.as_root(|| {
                    run_cmd(isol.command("resize2fs")?.arg(&output_abs_path))
                        .context("failed to resize ext3 archive")
                })??;
            }
        };

        Ok(())
    }
}
