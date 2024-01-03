/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::ffi::OsStr;
use std::process::Command;

use derive_more::From;
use isolate_cfg::IsolationContext;

use crate::Result;

#[derive(Debug, From)]
#[from(forward)]
#[repr(transparent)]
pub struct IsolatedContext<'a>(IsolatedContextInner<'a>);

impl<'a> IsolatedContext<'a> {
    pub fn command<S: AsRef<OsStr>>(&self, program: S) -> Result<Command> {
        self.0.command(program)
    }
}

#[derive(Debug, From)]
enum IsolatedContextInner<'a> {
    #[cfg(target_os = "linux")]
    Nspawn(isolate_nspawn::IsolatedContext),
    #[cfg(target_os = "linux")]
    Unshare(isolate_unshare::IsolatedContext<'a>),
}

impl<'a> IsolatedContextInner<'a> {
    pub fn command<S: AsRef<OsStr>>(&self, program: S) -> Result<Command> {
        match self {
            #[cfg(target_os = "linux")]
            Self::Nspawn(ctx) => Ok(ctx.command(program)),
            #[cfg(target_os = "linux")]
            Self::Unshare(ctx) => ctx.command(program).map_err(crate::Error::from),
        }
    }
}

#[cfg(target_os = "linux")]
pub fn nspawn(ctx: IsolationContext) -> Result<IsolatedContext> {
    Ok(isolate_nspawn::nspawn(ctx).into())
}

#[cfg(target_os = "linux")]
pub fn unshare(ctx: IsolationContext) -> Result<IsolatedContext> {
    Ok(isolate_unshare::prepare(ctx).into())
}

#[cfg(not(target_os = "linux"))]
compile_error!("only supported on linux");
