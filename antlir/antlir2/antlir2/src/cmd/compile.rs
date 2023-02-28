/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompileFeature;
use clap::Parser;

use super::Compileish;
use crate::Result;

#[derive(Parser, Debug)]
/// Compile image features into a directory
pub(crate) struct Compile {
    #[clap(flatten)]
    pub(super) compileish: Compileish,
}

impl Compile {
    #[tracing::instrument(name = "compile", skip(self))]
    pub(crate) fn run(self) -> Result<()> {
        let ctx = self.compileish.compiler_context()?;
        for feature in self.compileish.external.depgraph.pending_features() {
            feature.compile(&ctx)?;
        }
        Ok(())
    }
}
