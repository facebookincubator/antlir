# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")

def _make_test_cmd(ctx: AnalysisContext) -> cmd_args:
    features = ctx.attrs.features[FeatureInfo]
    features_json = ensure_single_output(ctx.attrs.features)

    hidden_deps = []

    # traverse the features to find dependencies this image build has on other
    # image layers
    dependency_layers = []
    for feat in features.features:
        for layer in feat.analysis.required_layers:
            if layer not in dependency_layers:
                dependency_layers.append(layer)

        hidden_deps.extend([feat.plugin.plugin, feat.plugin.libs])

    return cmd_args(
        ctx.attrs.test_depgraph[RunInfo],
        cmd_args(str(ctx.label), format = "--label={}"),
        cmd_args(features_json, format = "--feature-json={}"),
        cmd_args(
            [li.depgraph for li in dependency_layers],
            format = "--image-dependency={}",
        ),
        cmd_args(ctx.attrs.error_regex, format = "--error-regex={}"),
        cmd_args(ctx.attrs.parent[LayerInfo].depgraph, format = "--parent={}") if ctx.attrs.parent else cmd_args(),
    ).hidden(hidden_deps)

def _bad_impl(ctx: AnalysisContext) -> list[Provider]:
    cmd = _make_test_cmd(ctx)
    return [
        DefaultInfo(),
        RunInfo(args = cmd),
        ExternalRunnerTestInfo(
            command = [cmd],
            type = "custom",
            run_from_project_root = True,
        ),
    ]

_bad_depgraph = rule(
    impl = _bad_impl,
    attrs = {
        "error_regex": attrs.string(),
        "features": attrs.dep(providers = [FeatureInfo]),
        "parent": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "test_depgraph": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_depgraph/tests/test_depgraph:test-depgraph")),
    },
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
    _bad_depgraph(
        name = name,
        features = ":" + name + "--features",
        **kwargs
    )

def _good_impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        DefaultInfo(),
        ExternalRunnerTestInfo(
            # force the layer to be built for the test to be considered a
            # success
            command = [cmd_args("true").hidden([ctx.attrs.layer[LayerInfo].subvol_symlink])],
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
    )
