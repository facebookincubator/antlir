/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use clap::Parser;
use tracing::debug;

use crate::Result;

#[derive(Parser, Debug)]
/// Run a unit test inside an image layer.
pub(crate) struct Test {
    #[clap(long)]
    /// Path to layer to run the test in
    layer: PathBuf,
    #[clap(long, default_value = "root")]
    /// Run the test as this user
    user: String,
    /// Test command
    test_cmd: Vec<OsString>,
}

impl Test {
    #[tracing::instrument(name = "test", skip(self))]
    pub(crate) fn run(self) -> Result<()> {
        let repo = find_root::find_repo_root(
            &absolute_path::AbsolutePathBuf::canonicalize(
                std::env::current_exe().context("while getting argv[0]")?,
            )
            .context("argv[0] not absolute")?,
        )
        .context("while looking for repo root")?;
        // Running a test is a pretty orthogonal use case to running the
        // compiler, even though they both go through similar isolation
        // mechanisms. It is intentional that 'antlir2 test' DOES NOT use
        // 'antlir2_isolate_compiler', to avoid polluting the compiler isolation
        // with details related only to running tests, and vice versa.
        let mut cmd = Command::new("sudo");
        cmd.arg("systemd-nspawn")
            .arg("--quiet")
            .arg("--directory")
            .arg(std::fs::canonicalize(&self.layer).context("while canonicalizing subvol path")?)
            .arg("--ephemeral")
            .arg("--as-pid2")
            .arg("--register=no")
            .arg("--keep-unit")
            .arg("--private-network")
            .arg("--chdir")
            .arg(std::env::current_dir().context("while getting cwd")?)
            .arg("--bind-ro")
            .arg(repo.as_ref())
            .arg("--user")
            .arg(&self.user);
        #[cfg(facebook)]
        {
            cmd.arg("--bind-ro").arg("/usr/local/fbcode");
            cmd.arg("--bind-ro").arg("/mnt/gvfs");
        }
        cmd.arg("--").args(self.test_cmd);

        debug!("executing test in isolated container: {cmd:?}");

        return Err(anyhow::anyhow!("failed to exec test: {:?}", cmd.exec()).into());
    }
}
