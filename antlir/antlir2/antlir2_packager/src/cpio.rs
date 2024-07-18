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
pub struct Cpio {
    build_appliance: BuildAppliance,
    layer: PathBuf,
}

impl PackageFormat for Cpio {
    fn build(&self, out: &Path) -> Result<()> {
        File::create(out).context("failed to create output file")?;

        let isol_context = IsolationContext::builder(self.build_appliance.path())
            .ephemeral(false)
            .readonly()
            .tmpfs(Path::new("/__antlir2__/out"))
            .outputs(("/__antlir2__/out/cpio", out))
            .inputs((Path::new("/__antlir2__/root"), self.layer.as_path()))
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"))
            .build();

        let cpio_script = "set -ue -o pipefail; \
            pushd /__antlir2__/root; \
            /usr/bin/find . -mindepth 1 ! -type s | \
            LANG=C /usr/bin/sort | \
            LANG=C /usr/bin/cpio -o -H newc \
            > /__antlir2__/out/cpio";

        run_cmd(
            unshare(isol_context)?
                .command("/bin/bash")?
                .arg("-c")
                .arg(cpio_script)
                .stdout(Stdio::piped()),
        )
        .context("Failed to build cpio archive")?;

        Ok(())
    }
}
