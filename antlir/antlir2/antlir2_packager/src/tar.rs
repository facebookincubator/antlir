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

use antlir2_isolate::isolate;
use antlir2_isolate::IsolationContext;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

use crate::run_cmd;
use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
pub struct Tar {
    build_appliance: PathBuf,
    layer: PathBuf,
}

impl PackageFormat for Tar {
    fn build(&self, out: &Path) -> Result<()> {
        File::create(&out).context("failed to create output file")?;

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

        let tar_script = format!(
            "tar -c \
                --sparse \
                --one-file-system \
                --acls \
                --xattrs \
                --to-stdout \
                -C \
                {} \
                . \
                > {}",
            layer_abs_path.display(),
            output_abs_path.as_path().display(),
        );

        run_cmd(
            isolate(isol_context)?
                .command("/bin/bash")?
                .arg("-c")
                .arg(tar_script)
                .stdout(Stdio::piped()),
        )
        .context("Failed to build tar")?;

        Ok(())
    }
}
