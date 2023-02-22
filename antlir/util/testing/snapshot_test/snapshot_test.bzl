# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:paths.bzl", "paths")

def _dir_snapshot_test_impl(ctx: "context") -> ["provider"]:
    inputs = ctx.actions.declare_output("inputs", dir = True)
    input_dir_map = {"actual": ctx.attrs.actual}
    for src in ctx.attrs.snapshot:
        input_dir_map[paths.join("snapshot", src.basename)] = src
    ctx.actions.copied_dir(inputs, input_dir_map)

    extra_args = []
    if not ctx.attrs.file_modes:
        extra_args += ["-G."]

    cmd = cmd_args(
        "git",
        "diff",
        "--color=always",
        "--no-index",
        cmd_args(extra_args),
        cmd_args(inputs, format = "{}/snapshot"),
        cmd_args(inputs, format = "{}/actual"),
    ).hidden(inputs)
    return [
        DefaultInfo(default_outputs = [inputs]),
        ExternalRunnerTestInfo(
            command = [cmd],
            type = "custom",
        ),
    ]

dir_snapshot_test = rule(
    impl = _dir_snapshot_test_impl,
    attrs = {
        "actual": attrs.source(doc = "freshly-built directory"),
        "file_modes": attrs.bool(doc = "compare file modes in addition to contents", default = True),
        "snapshot": attrs.list(attrs.source(doc = "expected directory contents")),
    },
    doc = "Simple unit test to ensure that a built directory has exactly some known contents",
)

def _file_snapshot_test_impl(ctx: "context") -> ["provider"]:
    inputs = ctx.actions.declare_output("inputs", dir = True)
    ctx.actions.copied_dir(inputs, {
        "actual": ctx.attrs.actual,
        "snapshot": ctx.attrs.snapshot,
    })

    extra_args = []
    if not ctx.attrs.mode:
        extra_args += ["-G."]

    cmd = cmd_args(
        "git",
        "diff",
        "--color=always",
        "--no-index",
        cmd_args(extra_args),
        cmd_args(inputs, format = "{}/snapshot"),
        cmd_args(inputs, format = "{}/actual"),
    ).hidden(inputs)
    return [
        DefaultInfo(default_outputs = [inputs]),
        ExternalRunnerTestInfo(
            command = [cmd],
            type = "custom",
        ),
    ]

file_snapshot_test = rule(
    impl = _file_snapshot_test_impl,
    attrs = {
        "actual": attrs.source(doc = "freshly-built file"),
        "mode": attrs.bool(doc = "compare file mode in addition to contents", default = True),
        "snapshot": attrs.source(doc = "expected file contents"),
    },
    doc = "Simple unit test to ensure that a built file has exactly some known contents",
)
