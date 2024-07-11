# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    deps_dir = {}
    for d in ctx.attrs.deps:
        starlark = ensure_single_output(d)
        deps_dir[str(d.label.raw_target()).replace("//", "/")] = starlark

    deps_dir = ctx.actions.copied_dir("deps", deps_dir)
    test_cmd = [
        ctx.attrs._runner[RunInfo],
        cmd_args(ctx.attrs.srcs, format = "--test={}"),
        cmd_args(deps_dir, format = "--deps={}"),
        cmd_args(ctx.label.cell, format = "--default-cell={}"),
        "--",
    ]

    return [
        DefaultInfo(sub_targets = {"dir": [DefaultInfo(deps_dir)]}),
        RunInfo(cmd_args(test_cmd)),
        ExternalRunnerTestInfo(
            type = "rust",
            command = test_cmd,
        ),
    ]

_starlark_unittest = rule(
    impl = _impl,
    attrs = {
        "deps": attrs.list(attrs.dep()),
        "srcs": attrs.list(attrs.source()),
        "_runner": attrs.default_only(attrs.exec_dep(default = "antlir//antlir/bzl/starlark_unittest:starlark-unittest")),
    },
)

starlark_unittest = rule_with_default_target_platform(_starlark_unittest)
