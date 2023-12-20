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

use antlir2_isolate::nspawn;
use antlir2_isolate::IsolationContext;
use anyhow::Context;
use anyhow::Result;
use serde::Deserialize;

use crate::run_cmd;
use crate::PackageFormat;

#[derive(Debug, Clone, Deserialize)]
pub struct Cpio {
    build_appliance: PathBuf,
    layer: PathBuf,
}

impl PackageFormat for Cpio {
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

        let cpio_script = format!(
            "set -ue -o pipefail; \
                pushd '{}'; \
                /usr/bin/find . -mindepth 1 ! -type s | \
                LANG=C /usr/bin/sort | \
                LANG=C /usr/bin/cpio -o -H newc \
                > {}",
            layer_abs_path.display(),
            output_abs_path.as_path().display()
        );

        run_cmd(
            nspawn(isol_context)?
                .command("/bin/bash")?
                .arg("-c")
                .arg(cpio_script)
                .stdout(Stdio::piped()),
        )
        .context("Failed to build cpio archive")?;

        Ok(())
    }
}
