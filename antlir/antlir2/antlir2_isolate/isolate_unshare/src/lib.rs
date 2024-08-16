/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

#[cfg(not(target_os = "linux"))]
compile_error!("only supported on linux");

use std::ffi::OsStr;
use std::process::Command;

use isolate_cfg::InvocationType;
use isolate_cfg::IsolationContext;

pub mod mount;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("unsupported setting: {0}")]
    UnsupportedSetting(&'static str),
    #[error(transparent)]
    IO(#[from] std::io::Error),
    #[error("parsing user database: {0}")]
    UserDb(#[from] antlir2_users::Error),
    #[error("user '{0}' not found in user database")]
    MissingUser(String),
    #[error("failed to serialize isolation settings: {0} - {1}")]
    Serialize(serde_json::Error, String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct IsolatedContext<'a>(IsolationContext<'a>);

impl<'a> IsolatedContext<'a> {
    pub fn command<S: AsRef<OsStr>>(&self, program: S) -> Result<Command> {
        // TODO: remove these settings entirely when we get rid of
        // systemd-nspawn / move the things that require this (like image_test)
        // to *only* use systemd-nspawn
        if self.0.invocation_type != InvocationType::Pid2Pipe {
            return Err(Error::UnsupportedSetting("invocation_type"));
        }
        if self.0.register {
            return Err(Error::UnsupportedSetting("register"));
        }
        if self.0.enable_network {
            return Err(Error::UnsupportedSetting("enable_network"));
        }

        let mut cmd = Command::new(
            buck_resources::get("antlir/antlir2/antlir2_isolate/isolate_unshare/preexec")
                .expect("isolate_unshare_preexec is always present"),
        );

        cmd.arg(
            serde_json::to_string(&self.0)
                .map_err(|e| Error::Serialize(e, format!("{:#?}", self.0)))?,
        );
        cmd.arg(program);
        cmd.arg("--");

        Ok(cmd)
    }
}

#[deny(unused_variables)]
pub fn prepare(ctx: IsolationContext) -> IsolatedContext {
    IsolatedContext(ctx)
}
