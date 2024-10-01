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
pub struct Squashfs {
    build_appliance: BuildAppliance,
    compressor: Option<String>,
    force_uid: Option<u32>,
    force_gid: Option<u32>,
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

        let mut mksquashfs = unshare(isol_context)?.command("/usr/sbin/mksquashfs")?;

        // Base options
        mksquashfs
            .arg("/__antlir2__/root")
            .arg("/__antlir2__/out/squashfs")
            .arg("-noappend")
            .arg("-one-file-system");

        // Options from the rule
        if let Some(compressor) = &self.compressor {
            mksquashfs.arg("-comp").arg(compressor);
        }
        if let Some(force_uid) = &self.force_uid {
            mksquashfs.arg("-force-uid").arg(force_uid.to_string());
        }
        if let Some(force_gid) = &self.force_gid {
            mksquashfs.arg("-force-gid").arg(force_gid.to_string());
        }

        run_cmd(&mut mksquashfs).context("Failed to build squashfs")?;

        Ok(())
    }
}
