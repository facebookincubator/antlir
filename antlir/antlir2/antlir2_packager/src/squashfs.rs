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
pub struct Squashfs {
    build_appliance: BuildAppliance,
}

impl PackageFormat for Squashfs {
    fn build(&self, out: &Path, layer: &Path) -> Result<()> {
        File::create(out).context("failed to create output file")?;

        let isol_context = IsolationContext::builder(self.build_appliance.path())
            .ephemeral(false)
            .readonly()
            .tmpfs(Path::new("/__antlir2__/out"))
            .outputs(("/__antlir2__/out/squashfs", out))
            .inputs((Path::new("/__antlir2__/root"), layer))
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"))
            .build();

        run_cmd(
            unshare(isol_context)?
                .command("/usr/sbin/mksquashfs")?
                .arg("/__antlir2__/root")
                .arg("/__antlir2__/out/squashfs")
                .arg("-comp")
                .arg("zstd")
                .arg("-noappend")
                .arg("-one-file-system")
                .stdout(Stdio::piped()),
        )
        .context("Failed to build squashfs")?;

        Ok(())
    }
}
