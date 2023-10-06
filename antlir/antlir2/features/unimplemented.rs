/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::requires_provides::Requirement;
use antlir2_features as _;
use anyhow as _;
use serde::Deserialize;
use serde::Serialize;
use tracing as _;

pub type Feature = Unimplemented;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Deserialize, Serialize)]
pub struct Unimplemented;

impl antlir2_depgraph::requires_provides::RequiresProvides for Unimplemented {
    fn provides(&self) -> Result<Vec<Item<'static>>, String> {
        unimplemented!()
    }

    fn requires(&self) -> Result<Vec<Requirement<'static>>, String> {
        unimplemented!()
    }
}

impl antlir2_compile::CompileFeature for Unimplemented {
    fn compile(&self, _ctx: &CompilerContext) -> antlir2_compile::Result<()> {
        unimplemented!()
    }
}
