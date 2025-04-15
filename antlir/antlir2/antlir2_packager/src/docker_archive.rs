/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::fs::File;
use std::path::Path;
use std::path::PathBuf;

use antlir2_isolate::IsolationContext;
use antlir2_isolate::unshare;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

use crate::BuildAppliance;
use crate::run_cmd;

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DockerArchive {
    build_appliance: BuildAppliance,
    oci: PathBuf,
}

impl DockerArchive {
    pub(crate) fn build(&self, out: &Path) -> Result<()> {
        File::create(out).context("failed to create output file")?;

        let isol_context = IsolationContext::builder(self.build_appliance.path())
            .ephemeral(false)
            .readonly()
            .tmpfs(Path::new("/__antlir2__/out"))
            .outputs(("/__antlir2__/out/docker_archive", out))
            .inputs((
                PathBuf::from("/__antlir2__/working_directory"),
                std::env::current_dir()?,
            ))
            .working_directory(Path::new("/__antlir2__/working_directory"))
            .build();

        let mut oci_arg: OsString = "oci:".into();
        oci_arg.push(self.oci.as_os_str());

        run_cmd(
            unshare(isol_context)?
                .command("skopeo")?
                .arg("copy")
                .arg(oci_arg)
                .arg("docker-archive:/__antlir2__/out/docker_archive"),
        )
        .context("Failed to convert to docker-archive")?;

        Ok(())
    }
}
