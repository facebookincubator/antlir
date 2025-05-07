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
pub struct Tar {
    build_appliance: BuildAppliance,
    preserve_xattrs: bool,
}

impl PackageFormat for Tar {
    fn build(&self, out: &Path, layer: &Path) -> Result<()> {
        File::create(out).context("failed to create output file")?;

        let isol_context = IsolationContext::builder(self.build_appliance.path())
            .ephemeral(false)
            .readonly()
            .tmpfs(Path::new("/__antlir2__/out"))
            .outputs(("/__antlir2__/out/tar", out))
            .inputs((Path::new("/__antlir2__/root"), layer))
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"))
            .build();

        let mut tar_cmd = unshare(isol_context)?.command("tar")?;
        tar_cmd
            .arg("--sparse")
            .arg("--one-file-system")
            .arg("--acls")
            // Sorted by name to ensure reproducibility, as well as
            // predictable ordering when the tar is read as a byte stream.
            // Some use cases require consumption of tar's contents with a
            // known ordering, such as when the tar contains incremental
            // btrfs snapshots that may be opportunistically skipped.
            .arg("--sort=name")
            .arg("-C")
            .arg("/__antlir2__/root")
            .arg("-c")
            .arg("-f")
            .arg("/__antlir2__/out/tar")
            .arg(".")
            .stdout(Stdio::piped());
        if self.preserve_xattrs {
            tar_cmd.arg("--xattrs");
        };

        run_cmd(&mut tar_cmd).context("Failed to build tar")?;

        Ok(())
    }
}
