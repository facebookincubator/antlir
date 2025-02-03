/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::path::PathBuf;

use anyhow::Result;
use clap::Parser;
use image_test_lib::Test;
use json_arg::JsonFile;

use crate::runtime;
use crate::spawn_common;

/// Run a unit test inside an image layer.
#[derive(Parser, Debug)]
pub(crate) struct Args {
    #[clap(long)]
    spec: JsonFile<runtime::Spec>,
    #[clap(long)]
    chdir_project_root: Option<PathBuf>,
    #[clap(subcommand)]
    test: Test,
}

impl Args {
    pub(crate) fn run(self) -> Result<()> {
        if let Some(chdir) = self.chdir_project_root {
            std::env::set_current_dir(chdir)?;
        }
        spawn_common::run()
            .spec(self.spec.into_inner())
            .test(self.test)
            .interactive(true)
            .call()
    }
}
