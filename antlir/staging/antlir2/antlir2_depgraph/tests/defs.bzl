# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("//antlir/buck2/bzl/feature:feature.bzl", "FeatureInfo", "feature")

DepgraphInfo = provider(fields = ["json"])

def _make_test_cmd(ctx: "context", expect, other_args = []) -> "cmd_args":
    return cmd_args(
        ctx.attrs.test_depgraph[RunInfo],
        ctx.attrs.features[FeatureInfo].json_files.project_as_args("feature_json"),
        "--expect",
        json.encode(expect),
        cmd_args(ctx.attrs.parent[DepgraphInfo].json, format = "--parent={}") if ctx.attrs.parent else cmd_args(),
        *other_args
    )

def _bad_impl(ctx: "context") -> ["provider"]:
    cmd = _make_test_cmd(ctx, {"err": ctx.attrs.error})
    return [
        DefaultInfo(),
        RunInfo(args = cmd),
        ExternalRunnerTestInfo(
            command = [cmd],
            type = "custom",
        ),
    ]

_bad_depgraph = rule(
    impl = _bad_impl,
    attrs = {
        "error": attrs.any(),
        "features": attrs.dep(providers = [FeatureInfo]),
        "parent": attrs.option(attrs.dep(providers = [DepgraphInfo]), default = None),
        "test_depgraph": attrs.default_only(attrs.exec_dep(default = "//antlir/staging/antlir2/antlir2_depgraph/tests/test_depgraph:test-depgraph")),
    },
)

def bad_depgraph(
        name: str.type,
        features,
        error,
        parent: [str.type, None] = None):
    feature(
        name = name + "--features",
        features = features,
        visibility = [":" + name],
    )
    _bad_depgraph(
        name = name,
        features = ":" + name + "--features",
        error = error,
        parent = parent,
    )

def _good_impl(ctx: "context") -> ["provider"]:
    out = ctx.actions.declare_output("depgraph.json")
    cmd = _make_test_cmd(ctx, {"ok": None}, [
        cmd_args(out.as_output(), format = "--out={}"),
    ])
    ctx.actions.run(cmd, category = "antlir2_depgraph")
    return [
        DefaultInfo(),
        DepgraphInfo(json = out),
        RunInfo(args = cmd),
        ExternalRunnerTestInfo(
            command = [cmd],
            type = "custom",
        ),
    ]

_good_depgraph = rule(
    impl = _good_impl,
    attrs = {
        "features": attrs.dep(providers = [FeatureInfo]),
        "parent": attrs.option(attrs.dep(providers = [DepgraphInfo]), default = None),
        "test_depgraph": attrs.default_only(attrs.exec_dep(default = "//antlir/staging/antlir2/antlir2_depgraph/tests/test_depgraph:test-depgraph")),
    },
)

def good_depgraph(
        name: str.type,
        features,
        parent: [str.type, None] = None):
    feature(
        name = name + "--features",
        features = features,
        visibility = [":" + name],
    )
    _good_depgraph(
        name = name,
        features = ":" + name + "--features",
        parent = parent,
    )
