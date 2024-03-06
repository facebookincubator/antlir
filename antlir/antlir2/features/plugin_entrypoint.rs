/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompileFeature as _;
use antlir2_compile::CompilerContext;
use antlir2_depgraph::requires_provides::RequiresProvides as _;
use anyhow::Context;
use r#impl::Feature;

#[no_mangle]
pub fn init_tracing(dispatch: &tracing::Dispatch) {
    let _ = tracing::dispatcher::set_global_default(dispatch.clone());
    tracing_core::callsite::rebuild_interest_cache();
}

static_assertions::assert_impl_all!(
    Feature: antlir2_depgraph::requires_provides::RequiresProvides, antlir2_compile::CompileFeature,
);

#[allow(non_snake_case)]
#[no_mangle]
pub fn RequiresProvides_provides(
    feature: &antlir2_features::Feature,
) -> std::result::Result<Vec<antlir2_depgraph::item::Item>, String> {
    let feature: Feature = serde_json::from_value(feature.data.clone())
        .map_err(|e| format!("failed to convert to deserialize specific feature type: {e}"))?;
    feature.provides()
}

#[allow(non_snake_case)]
#[no_mangle]
pub fn RequiresProvides_requires(
    feature: &antlir2_features::Feature,
) -> std::result::Result<Vec<antlir2_depgraph::requires_provides::Requirement>, String> {
    let feature: Feature = serde_json::from_value(feature.data.clone())
        .map_err(|e| format!("failed to convert to deserialize specific feature type: {e}"))?;
    feature.requires()
}

#[allow(non_snake_case)]
#[no_mangle]
fn CompileFeature_compile(
    feature: &antlir2_features::Feature,
    ctx: &CompilerContext,
) -> antlir2_compile::Result<()> {
    let feature: Feature = serde_json::from_value(feature.data.clone())
        .context("while deserializing to specific feature type")?;
    feature.compile(ctx)
}

#[allow(non_snake_case)]
#[no_mangle]
pub fn CompileFeature_plan(
    feature: &antlir2_features::Feature,
    ctx: &CompilerContext,
) -> antlir2_compile::Result<Vec<antlir2_compile::plan::Item>> {
    let feature: Feature = serde_json::from_value(feature.data.clone())
        .context("while deserializing to specific feature type")?;
    feature.plan(ctx)
}
