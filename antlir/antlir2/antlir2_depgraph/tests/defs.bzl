# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:platform.bzl", "default_target_platform_kwargs")
load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/bzl/image:depgraph.bzl", "analyze_features")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo", "FeaturePluginPluginKind")
load("//antlir/bzl:build_defs.bzl", "buck_sh_test")

def _bad_impl(ctx: AnalysisContext) -> list[Provider]:
    features = ctx.attrs.features[FeatureInfo]

    analyzed_features = analyze_features(
        ctx = ctx,
        features = features.features,
        identifier = "depgraph_test",
        phase = BuildPhase("compile"),
        plugins = {str(plugin.label.raw_target()): plugin[FeaturePluginInfo] for plugin in ctx.plugins[FeaturePluginPluginKind]},
    )

    cmd = cmd_args(
        ctx.attrs.test_depgraph[RunInfo],
        cmd_args(analyzed_features, format = "--feature={}"),
        cmd_args(ctx.attrs.error_regex, format = "--error-regex={}"),
        cmd_args(ctx.attrs.parent[LayerInfo].facts_db, format = "--parent={}") if ctx.attrs.parent else cmd_args(),
    )
    return [
        DefaultInfo(),
        RunInfo(args = cmd),
    ]

_bad_depgraph_test_runner = rule(
    impl = _bad_impl,
    attrs = {
        "error_regex": attrs.string(),
        "features": attrs.dep(
            providers = [FeatureInfo],
            pulls_plugins = [FeaturePluginPluginKind],
        ),
        "parent": attrs.option(
            attrs.dep(providers = [LayerInfo]),
            default = None,
        ),
        "test_depgraph": attrs.default_only(attrs.dep(default = "//antlir/antlir2/antlir2_depgraph/tests/test_depgraph:test-depgraph")),
        "_analyze_feature": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_depgraph_if:analyze")),
    },
    uses_plugins = [FeaturePluginPluginKind],
)

def bad_depgraph(
        name: str,
        features,
        **kwargs):
    feature.new(
        name = name + "--features",
        features = features,
        visibility = [":" + name],
    )
    _bad_depgraph_test_runner(
        name = name + "--test",
        features = ":" + name + "--features",
        **(kwargs | default_target_platform_kwargs())
    )
    buck_sh_test(
        name = name,
        test = ":" + name + "--test",
    )

def _good_impl(ctx: AnalysisContext) -> list[Provider]:
    layer_contents = ctx.attrs.layer[LayerInfo].contents
    return [
        DefaultInfo(),
        ExternalRunnerTestInfo(
            # force the layer to be built for the test to be considered a
            # success
            command = [cmd_args("true", hidden = [layer_contents.subvol_symlink])],
            default_executor = CommandExecutorConfig(
                local_enabled = True,
                # Requires local subvolume and cannot be run on RE
                remote_enabled = False,
            ),
            type = "custom",
        ),
    ]

_good_depgraph = rule(
    impl = _good_impl,
    attrs = {
        "layer": attrs.dep(providers = [LayerInfo]),
    },
)

def good_depgraph(name, **kwargs):
    image.layer(name = name, **kwargs)
    _good_depgraph(
        name = name + "-test",
        layer = ":" + name,
        **default_target_platform_kwargs()
    )
