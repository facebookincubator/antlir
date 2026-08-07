/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

use crate::BuildAppliance;
use crate::PackageFormat;
use crate::run_cmd;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Erofs {
    build_appliance: BuildAppliance,
    label: Option<String>,
    compression: Option<String>,
    fixed_metadata: bool,
}

/// Fixed timestamp for reproducible erofs images.
/// February 4, 2004 - the initial launch of thefacebook.com.
/// This matches REPRODUCIBLE_SOURCE_DATE_EPOCH in the ext4 packager.
const REPRODUCIBLE_SOURCE_DATE_EPOCH: &str = "1075852800";

/// Fixed filesystem UUID for reproducible erofs images. Matches
/// REPRODUCIBLE_FS_UUID in the ext4 packager.
const REPRODUCIBLE_FS_UUID: &str = "00000000-0000-4000-8000-000000000000";

impl PackageFormat for Erofs {
    fn build(&self, out: &Path, layer: &Path) -> Result<()> {
        File::create(out).context("failed to create output file")?;

        let mut binding = IsolationContext::builder(self.build_appliance.path());
        let isol_context = binding
            .ephemeral(false)
            .readonly()
            .tmpfs(Path::new("/__antlir2__/out"))
            .outputs(("/__antlir2__/out/erofs", out))
            .inputs((Path::new("/__antlir2__/root"), layer))
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"));

        // mkfs.erofs stamps the current time into the superblock unless
        // SOURCE_DATE_EPOCH is set, so without this two builds of an identical
        // layer differ in the superblock (and its checksum).
        let isol_context = if self.fixed_metadata {
            isol_context
                .setenv(("SOURCE_DATE_EPOCH", REPRODUCIBLE_SOURCE_DATE_EPOCH))
                .build()
        } else {
            isol_context.build()
        };

        let mut cmd = unshare(isol_context)?.command("mkfs.erofs")?;
        cmd.arg("/__antlir2__/out/erofs").arg("/__antlir2__/root");
        if self.fixed_metadata {
            // Otherwise mkfs.erofs generates a random UUID on every run.
            cmd.arg("-U").arg(REPRODUCIBLE_FS_UUID);
        }
        if let Some(compression) = &self.compression {
            cmd.arg("-z").arg(compression);
        }
        if let Some(label) = &self.label {
            cmd.arg("-L").arg(label);
        }

        run_cmd(&mut cmd).context("while running mkfs.erofs")?;

        Ok(())
    }
}
