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
use std::path::Path;
use std::path::PathBuf;
use std::process::Stdio;

use antlir2_isolate::isolate;
use antlir2_isolate::IsolationContext;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

use crate::run_cmd;
use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
pub struct Vfat {
    build_appliance: PathBuf,
    layer: PathBuf,
    fat_size: Option<u16>,
    label: Option<String>,
    size_mb: u64,
}

impl PackageFormat for Vfat {
    fn build(&self, out: &Path) -> Result<()> {
        let mut file = File::create(&out).context("failed to create output file")?;
        file.seek(SeekFrom::Start(self.size_mb * 1024 * 1024))
            .context("failed to seek output to specified size")?;
        file.write_all(&[0])
            .context("Failed to write dummy byte at end of file")?;
        file.sync_all()
            .context("Failed to sync output file to disk")?;
        drop(file);

        let input = self
            .layer
            .canonicalize()
            .context("failed to build abs path to layer")?;

        let output = out
            .canonicalize()
            .context("failed to build abs path to output")?;

        let isol_context = IsolationContext::builder(&self.build_appliance)
            .inputs(input.as_path())
            .outputs(output.as_path())
            .setenv(("RUST_LOG", std::env::var_os("RUST_LOG").unwrap_or_default()))
            .setenv(("MTOOLS_SKIP_CHECK", "1"))
            .build();

        // Build the vfat disk file first
        let mut mkfs = isolate(isol_context.clone())?.command("/usr/sbin/mkfs.vfat")?;
        if let Some(fat_size) = &self.fat_size {
            mkfs.arg(format!("-F{}", fat_size));
        }
        if let Some(label) = &self.label {
            mkfs.arg("-n").arg(label);
        }
        mkfs.arg("-S").arg("4096");

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
}
