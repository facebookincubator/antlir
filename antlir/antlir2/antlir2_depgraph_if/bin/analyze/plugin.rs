/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use std::fmt::Debug;

use antlir2_depgraph_if::item::Item;
use antlir2_depgraph_if::Requirement;
use antlir2_depgraph_if::RequiresProvides;
use antlir2_features::Feature;
use anyhow::Context;
use anyhow::Result;

/// PluginExt indirects the implementation of [RequiresProvides] through a .so
/// plugin. The underlying crates all provide a type that implements
/// [RequiresProvides], and some generated code provides a set of exported
/// symbols that let us call that implementation.
trait PluginExt {
    fn as_requires_provides_fn(
        &self,
    ) -> Result<
        libloading::Symbol<fn(&Feature) -> antlir2_features::Result<Box<dyn RequiresProvides>>>,
    >;
}

impl PluginExt for antlir2_features::Plugin {
    fn as_requires_provides_fn(
        &self,
    ) -> Result<
        libloading::Symbol<fn(&Feature) -> antlir2_features::Result<Box<dyn RequiresProvides>>>,
    > {
        self.get_symbol(b"as_requires_provides\0")
            .context("while getting 'as_requires_provides' symbol")
    }
}

pub(crate) struct FeatureWrapper<'a>(pub(crate) &'a Feature);

impl<'a> Debug for FeatureWrapper<'a> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl<'a> RequiresProvides for FeatureWrapper<'a> {
    #[tracing::instrument]
    fn provides(&self) -> Result<Vec<Item>, String> {
        let func = self
            .0
            .plugin()
            .map_err(|e| e.to_string())?
            .as_requires_provides_fn()
            .map_err(|e| e.to_string())?;
        let feat = func(self.0).map_err(|e| e.to_string())?;
        feat.provides()
    }

    #[tracing::instrument]
    fn requires(&self) -> Result<Vec<Requirement>, String> {
        let func = self
            .0
            .plugin()
            .map_err(|e| e.to_string())?
            .as_requires_provides_fn()
            .map_err(|e| e.to_string())?;
        let feat = func(self.0).map_err(|e| e.to_string())?;
        feat.requires()
    }
}
