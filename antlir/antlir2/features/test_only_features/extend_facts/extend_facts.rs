/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::item::Item;
use antlir2_features as _;
use serde::Deserialize;
use serde::Serialize;
use tracing as _;

pub type Feature = ExtendFacts;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct ExtendFacts {
    msg: String,
}

impl antlir2_depgraph_if::RequiresProvides for ExtendFacts {
    fn provides(&self) -> Result<Vec<Item>, String> {
        Ok(Default::default())
    }

    #[deny(unused_variables)]
    fn requires(&self) -> Result<Vec<Requirement>, String> {
        Ok(Default::default())
    }
}

impl antlir2_compile::CompileFeature for ExtendFacts {
    fn compile(&self, _ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        Ok(())
    }
}
