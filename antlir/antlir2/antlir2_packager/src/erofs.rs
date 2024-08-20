/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

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
pub struct Erofs {
    build_appliance: BuildAppliance,
    label: Option<String>,
    compression: Option<String>,
}

impl PackageFormat for Erofs {
    fn build(&self, out: &Path, layer: &Path) -> Result<()> {
        File::create(out).context("failed to create output file")?;

        let isol_context = IsolationContext::builder(self.build_appliance.path())
            .ephemeral(false)
            .readonly()
            .tmpfs(Path::new("/__antlir2__/out"))
            .outputs(("/__antlir2__/out/erofs", out))
            .inputs((Path::new("/__antlir2__/root"), layer))
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"))
            .build();

        let mut cmd = unshare(isol_context)?.command("mkfs.erofs")?;
        cmd.arg("/__antlir2__/out/erofs").arg("/__antlir2__/root");
        if let Some(compression) = &self.compression {
            cmd.arg("-z").arg(compression);
        }
        if let Some(label) = &self.label {
            cmd.arg("-L").arg(label);
        }

        run_cmd(&mut cmd).context("Failed to build cpio archive")?;

        Ok(())
    }
}
