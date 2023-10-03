/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::process::Command;

use derive_more::From;
#[cfg(target_os = "linux")]
pub use isolate_bwrap::bwrap;
use isolate_cfg::IsolationContext;

use crate::Result;

#[derive(Debug, From)]
#[from(forward)]
#[repr(transparent)]
pub struct IsolatedContext(IsolatedContextInner);

impl IsolatedContext {
    pub fn command<S: AsRef<OsStr>>(&self, program: S) -> Result<Command> {
        self.0.command(program)
    }
}

#[derive(Debug, From)]
enum IsolatedContextInner {
    #[cfg(target_os = "linux")]
    Nspawn(isolate_nspawn::IsolatedContext),
    #[cfg(target_os = "linux")]
    Bwrap(isolate_bwrap::IsolatedContext),
}

impl IsolatedContextInner {
    pub fn command<S: AsRef<OsStr>>(&self, program: S) -> Result<Command> {
        match self {
            #[cfg(target_os = "linux")]
            Self::Nspawn(ctx) => Ok(ctx.command(program)),
            #[cfg(target_os = "linux")]
            Self::Bwrap(ctx) => Ok(ctx.command(program)),
        }
    }
}

#[cfg(target_os = "linux")]
pub fn isolate(ctx: IsolationContext) -> Result<IsolatedContext> {
    Ok(isolate_nspawn::nspawn(ctx).into())
}

#[cfg(not(target_os = "linux"))]
compile_error!("only supported on linux");
