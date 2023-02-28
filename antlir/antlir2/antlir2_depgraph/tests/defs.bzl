# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("//antlir/antlir2:antlir2_layer_info.bzl", "LayerInfo")
load("//antlir/buck2/bzl/feature:feature.bzl", "FeatureInfo", "feature")
load("//antlir/bzl:flatten.bzl", "flatten")

def _make_test_cmd(ctx: "context", expect) -> "cmd_args":
    # traverse the features to find dependencies this image build has on other
    # image layers
    dependency_layers = []
    for dep in flatten.flatten(ctx.attrs.features[FeatureInfo].deps.traverse()):
        if type(dep) == "dependency" and LayerInfo in dep:
            dependency_layers.append(dep[LayerInfo])

    return cmd_args(
        ctx.attrs.test_depgraph[RunInfo],
        cmd_args(str(ctx.label), format = "--label={}"),
        ctx.attrs.features[FeatureInfo].json_files.project_as_args("feature_json"),
        cmd_args(
            [li.depgraph for li in dependency_layers],
            format = "--image-dependency={}",
        ),
        "--expect",
        json.encode(expect),
        cmd_args(ctx.attrs.parent[LayerInfo].depgraph, format = "--parent={}") if ctx.attrs.parent else cmd_args(),
    )

def _bad_impl(ctx: "context") -> ["provider"]:
    if ctx.attrs.error:
        expect = {"err": ctx.attrs.error}
    elif ctx.attrs.error_regex:
        expect = {"error_regex": ctx.attrs.error_regex}
    else:
        fail("one of {error, error_regex} must be set")

    cmd = _make_test_cmd(ctx, expect)
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
        "error": attrs.option(attrs.any(), default = None),
        "error_regex": attrs.option(attrs.string(), default = None),
        "features": attrs.dep(providers = [FeatureInfo]),
        "parent": attrs.option(attrs.dep(providers = [LayerInfo]), default = None),
        "test_depgraph": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/antlir2_depgraph/tests/test_depgraph:test-depgraph")),
    },
)

def bad_depgraph(
        name: str.type,
        features,
        **kwargs):
    feature(
        name = name + "--features",
        features = features,
        visibility = [":" + name],
    )
    _bad_depgraph(
        name = name,
        features = ":" + name + "--features",
        **kwargs
    )
