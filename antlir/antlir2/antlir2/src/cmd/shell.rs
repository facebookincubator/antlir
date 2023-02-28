/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::os::unix::process::CommandExt;
use std::process::Command;

use anyhow::Error;
use clap::Parser;

use crate::Result;

#[derive(Parser, Debug)]
/// Run a shell inside the isolated context.
///
/// Why a subcommand? It makes it so that 'isolate.rs' only ever has to run
/// antlir2 itself, and is a great place to add additional features like
/// spawning the tcp proxy or printing help information before dropping the user
/// to a shell.
pub(crate) struct Shell {}

impl Shell {
    #[tracing::instrument(name = "shell", skip(self))]
    pub fn run(self) -> Result<()> {
        Err(Error::from(Command::new("/bin/sh").exec())
            .context("while execing isolated compiler")
            .into())
    }
}
