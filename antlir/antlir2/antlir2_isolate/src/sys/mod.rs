/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::collections::HashMap;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::process::Command;

use crate::Result;

#[cfg(target_os = "linux")]
mod bwrap;
#[cfg(target_os = "linux")]
mod nspawn;

#[cfg(target_os = "linux")]
pub use bwrap::bwrap;
#[cfg(target_os = "linux")]
pub use nspawn::nspawn as isolate;

#[cfg(target_os = "linux")]
#[derive(Debug)]
pub struct IsolatedContext {
    program: OsString,
    args: Vec<OsString>,
    env: HashMap<OsString, OsString>,
    #[allow(dead_code)]
    ephemeral_subvol: Option<bwrap::EphemeralSubvolume>,
}

#[cfg(target_os = "linux")]
impl IsolatedContext {
    pub fn command<S: AsRef<OsStr>>(&self, program: S) -> Result<Command> {
        let mut cmd = Command::new(&self.program);
        cmd.args(&self.args).arg("--").arg(program);
        for (k, v) in &self.env {
            cmd.env(k, v);
        }
        Ok(cmd)
    }
}

#[cfg(not(target_os = "linux"))]
compile_error!("only supported on linux");
