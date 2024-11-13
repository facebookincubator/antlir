/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::os::unix::process::CommandExt;
use std::path::PathBuf;
use std::process::Command;

use anyhow::Context;
use anyhow::Result;
use bon::Builder;
use clap::Parser;
use json_arg::JsonFile;
use nix::unistd::User;
use serde::Deserialize;
use serde::Serialize;

#[derive(Debug, Clone, Builder, Serialize, Deserialize)]
/// Specification of how to execute the test.
/// This specification is just how to invoke the inner test binary, the
/// containerization should already have been set up by 'spawn'.
pub(crate) struct Spec {
    /// The test command
    cmd: Vec<OsString>,
    /// CWD of the test
    working_directory: PathBuf,
    /// Run the test as this user
    user: String,
    /// Set these env vars in the test environment
    #[serde(default)]
    env: BTreeMap<String, String>,
}

#[derive(Debug, Parser)]
/// Execute the inner test
pub(crate) struct Args {
    #[clap(default_value = "/__antlir2_image_test__/exec_spec.json")]
    spec: JsonFile<Spec>,
}

impl Args {
    pub(crate) fn run(self) -> Result<()> {
        let spec = self.spec.into_inner();
        std::env::set_current_dir(&spec.working_directory)
            .with_context(|| format!("while changing to '{}'", spec.working_directory.display()))?;
        let mut env = spec.env;
        env.insert("USER".into(), spec.user.clone());
        env.insert(
            "PWD".into(),
            spec.working_directory
                .to_str()
                .with_context(|| {
                    format!("pwd '{}' was not utf8", spec.working_directory.display())
                })?
                .into(),
        );

        let user = User::from_name(&spec.user)
            .context("failed to lookup user")?
            .with_context(|| format!("no such user '{}'", spec.user))?;

        let mut cmd = spec.cmd.into_iter();
        let err = Command::new(cmd.next().context("test command was empty")?)
            .args(cmd)
            .envs(env)
            .uid(user.uid.into())
            .gid(user.gid.into())
            .exec();
        Err(err.into())
    }
}
