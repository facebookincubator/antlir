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

use antlir2_isolate::unshare;
use antlir2_isolate::IsolationContext;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

use crate::run_cmd;
use crate::BuildAppliance;
use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Vfat {
    build_appliance: BuildAppliance,
    layer: PathBuf,
    fat_size: Option<u16>,
    label: Option<String>,
    size_mb: u64,
}

impl PackageFormat for Vfat {
    fn build(&self, out: &Path) -> Result<()> {
        let file = File::create(out).context("failed to create output file")?;
        file.set_len(self.size_mb * 1024 * 1024)
            .context("failed to set output to specified size")?;
        file.sync_all()
            .context("Failed to sync output file to disk")?;
        drop(file);

        let isol_context = IsolationContext::builder(self.build_appliance.path())
            .ephemeral(false)
            .readonly()
            .tmpfs(Path::new("/__antlir2__/out"))
            .outputs(("/__antlir2__/out/vfat", out))
            .inputs((Path::new("/__antlir2__/root"), self.layer.as_path()))
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"))
            .setenv(("RUST_LOG", std::env::var_os("RUST_LOG").unwrap_or_default()))
            .setenv(("MTOOLS_SKIP_CHECK", "1"))
            .build();

        // Build the vfat disk file first
        let mut mkfs = unshare(isol_context.clone())?.command("/usr/sbin/mkfs.vfat")?;
        if let Some(fat_size) = &self.fat_size {
            mkfs.arg(format!("-F{}", fat_size));
        }
        if let Some(label) = &self.label {
            mkfs.arg("-n").arg(label);
        }
        mkfs.arg("-S").arg("4096");

        run_cmd(mkfs.arg("/__antlir2__/out/vfat").stdout(Stdio::piped()))
            .context("failed to mkfs.vfat")?;

        // mcopy all the files from the input layer directly into the vfat image.
        let paths = std::fs::read_dir(&self.layer).context("Failed to list input directory")?;
        let mut sources = Vec::new();
        for path in paths {
            sources.push(
                Path::new("/__antlir2__/root")
                    .join(path.context("failed to read next input path")?.file_name()),
            );
        }

        run_cmd(
            unshare(isol_context)?
                .command("/usr/bin/mcopy")?
                .arg("-v")
                .arg("-i")
                .arg("/__antlir2__/out/vfat")
                .arg("-sp")
                .args(sources)
                .arg("::")
                .stdout(Stdio::piped()),
        )
        .context("Failed to mcopy layer into new fs")?;

        Ok(())
    }
}
