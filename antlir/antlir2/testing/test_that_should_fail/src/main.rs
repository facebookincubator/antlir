/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsString;
use std::path::PathBuf;
use std::process::Command;

use anyhow::ensure;
use anyhow::Context;
use anyhow::Result;
use clap::Parser;
use regex::Regex;

#[derive(Parser, Debug)]
/// Run a test that should fail
struct Args {
    #[clap(long)]
    stdout_re: Option<Regex>,
    #[clap(long)]
    stderr_re: Option<Regex>,
    /// The test to run
    test_exe: PathBuf,
    /// Args to pass to the test
    test_cmd: Vec<OsString>,
}

fn main() -> Result<()> {
    let args = Args::parse();
    let res = Command::new(&args.test_exe)
        .args(&args.test_cmd)
        .output()
        .context("failed to execute test")?;
    ensure!(
        !res.status.success(),
        "test exited successfully, that is unexpected"
    );
    if let Some(re) = args.stdout_re {
        let stdout = std::str::from_utf8(&res.stdout).context("stdout not utf8")?;
        ensure!(re.is_match(stdout), "stdout did not match {re}:\n{stdout}");
    }
    if let Some(re) = args.stderr_re {
        let stderr = std::str::from_utf8(&res.stderr).context("stdout not utf8")?;
        ensure!(re.is_match(stderr), "stderr did not match {re}:\n{stderr}");
    }
    Ok(())
}
