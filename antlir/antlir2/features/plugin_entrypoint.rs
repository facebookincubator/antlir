/*
 * Copyright (c) Meta Platforms, Inc. and affiliates.
 *
 * This source code is licensed under the MIT license found in the
 * LICENSE file in the root directory of this source tree.
 */

use r#impl::Feature;

#[unsafe(no_mangle)]
pub extern "Rust" fn init_tracing(dispatch: tracing::Dispatch) {
    let _ = tracing::dispatcher::set_global_default(dispatch);
    tracing_core::callsite::rebuild_interest_cache();
}

static_assertions::assert_impl_all!(
    Feature: antlir2_depgraph_if::RequiresProvides, antlir2_compile::CompileFeature,
);

#[unsafe(no_mangle)]
pub extern "Rust" fn as_requires_provides(
    feature: &antlir2_features::Feature,
) -> antlir2_features::Result<Box<dyn antlir2_depgraph_if::RequiresProvides>> {
    let feature: Box<Feature> = serde_json::from_value(feature.data.clone())
        .map_err(antlir2_features::Error::Deserialize)?;
    Ok(feature)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn as_compile_feature(
    feature: &antlir2_features::Feature,
) -> antlir2_features::Result<Box<dyn antlir2_compile::CompileFeature>> {
    let feature: Box<Feature> = serde_json::from_value(feature.data.clone())
        .map_err(antlir2_features::Error::Deserialize)?;
    Ok(feature)
}

#[unsafe(no_mangle)]
pub extern "Rust" fn label() -> &'static str {
    env!("LABEL")
}
