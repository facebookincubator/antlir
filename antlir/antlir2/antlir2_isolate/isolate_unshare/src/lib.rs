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
    #[error("failed to serialize isolation settings: {0} - {1}")]
    Serialize(serde_json::Error, String),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

#[derive(Debug)]
pub struct IsolatedContext<'a>(IsolationContext<'a>);

impl<'a> IsolatedContext<'a> {
    pub fn command<S: AsRef<OsStr>>(&self, program: S) -> Result<Command> {
        if self.0.register {
            return Err(Error::UnsupportedSetting("register"));
        }
        // TODO: support this when we can bind the controlling terminal to
        // /dev/console, otherwise don't lie about providing an interactive
        // console
        if self.0.invocation_type == InvocationType::BootInteractive {
            return Err(Error::UnsupportedSetting("invocation_type=BootInteractive"));
        }

        let mut cmd = Command::new(
            buck_resources::get("antlir/antlir2/antlir2_isolate/isolate_unshare/preexec")
                .expect("isolate_unshare_preexec is always present"),
        );

        cmd.arg("main")
            .arg(
                serde_json::to_string(&self.0)
                    .map_err(|e| Error::Serialize(e, format!("{:#?}", self.0)))?,
            )
            .arg(program)
            .arg("--")
            // LSAN does not work under PID namespaces, so disable it
            .env("ASAN_OPTIONS", "detect_leaks=0");

        Ok(cmd)
    }
}

#[deny(unused_variables)]
pub fn prepare(ctx: IsolationContext) -> IsolatedContext {
    IsolatedContext(ctx)
}
