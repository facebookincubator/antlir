/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::plan;
use antlir2_compile::CompilerContext;
use antlir2_depgraph::item::Item;
use antlir2_depgraph::requires_provides::Requirement;
use anyhow::Result;

pub trait Feature<'f> {
    /// List of what [Item]s this [Feature] provides. Added to the graph before
    /// any [Requirement]s so that edges work.
    fn provides(&self) -> Result<Vec<Item<'f>>>;

    /// List of what [Item]s this [Feature] requires to be provided by other
    /// features / parent images.
    fn requires(&self) -> Result<Vec<Requirement<'f>>>;

    fn compile(&self, ctx: &CompilerContext) -> Result<()>;

    fn plan(&self, _ctx: &CompilerContext) -> Result<Vec<plan::Item>> {
        Ok(Default::default())
    }
}
