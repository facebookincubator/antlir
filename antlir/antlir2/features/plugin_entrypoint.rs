/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use antlir2_compile::CompilerContext;
use antlir2_feature_impl::Feature as _;
use anyhow::Context;
use r#impl::Feature;

#[no_mangle]
pub fn init_tracing(dispatch: &tracing::Dispatch) {
    let _ = tracing::dispatcher::set_global_default(dispatch.clone());
    tracing_core::callsite::rebuild_interest_cache();
}

#[allow(non_snake_case)]
#[no_mangle]
pub fn RequiresProvides_provides(
    feature: &antlir2_features::Feature,
) -> std::result::Result<Vec<antlir2_depgraph::item::Item<'static>>, String> {
    let feature: Feature = serde_json::from_value(feature.data.clone())
        .map_err(|e| format!("failed to convert to dserialize specific feature type: {e}"))?;
    feature.provides().map_err(|e| e.to_string())
}

#[allow(non_snake_case)]
#[no_mangle]
pub fn RequiresProvides_requires(
    feature: &antlir2_features::Feature,
) -> std::result::Result<Vec<antlir2_depgraph::requires_provides::Requirement<'static>>, String> {
    let feature: Feature = serde_json::from_value(feature.data.clone())
        .map_err(|e| format!("failed to convert to dserialize specific feature type: {e}"))?;
    feature.requires().map_err(|e| e.to_string())
}

#[allow(non_snake_case)]
#[no_mangle]
fn CompileFeature_compile(
    feature: &antlir2_features::Feature,
    ctx: &CompilerContext,
) -> antlir2_compile::Result<()> {
    let feature: Feature = serde_json::from_value(feature.data.clone())
        .context("while deserializing to specific feature type")?;
    feature.compile(ctx).map_err(antlir2_compile::Error::from)
}

#[allow(non_snake_case)]
#[no_mangle]
pub fn CompileFeature_plan(
    feature: &antlir2_features::Feature,
    ctx: &CompilerContext,
) -> antlir2_compile::Result<Vec<antlir2_compile::plan::Item>> {
    let feature: Feature = serde_json::from_value(feature.data.clone())
        .context("while deserializing to specific feature type")?;
    feature.plan(ctx).map_err(antlir2_compile::Error::from)
}
