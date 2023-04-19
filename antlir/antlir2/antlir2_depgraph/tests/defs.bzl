# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("//antlir/antlir2/bzl:types.bzl", "FeatureInfo", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/bzl:flatten.bzl", "flatten")

def _make_test_cmd(ctx: "context", expect) -> "cmd_args":
    features = ctx.attrs.features[FeatureInfo]
    features_json = features.features.project_as_json("features_json")
    features_json = ctx.actions.write_json("features.json", features_json, with_inputs = True)

    # traverse the features to find dependencies this image build has on other
    # image layers
    dependency_layers = flatten.flatten(list(features.required_layers.traverse()))

    return cmd_args(
        ctx.attrs.test_depgraph[RunInfo],
        cmd_args(str(ctx.label), format = "--label={}"),
        cmd_args(features_json, format = "--feature-json={}"),
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

def _good_impl(ctx: "context") -> ["provider"]:
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
