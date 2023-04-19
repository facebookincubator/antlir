# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@fbcode_macros//build_defs:fully_qualified_test_name_rollout.bzl", "NAMING_ROLLOUT_LABEL", "fully_qualified_test_name_rollout")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

def _impl(ctx: "context") -> ["provider"]:
    if not ctx.attrs.layer[LayerInfo].parent:
        fail("image_diff_test only works for layers with parents")
    base_cmd = cmd_args(
        "sudo",  # this needs to read files that are only readable by root (eg /etc/shadow)
        ctx.attrs.image_diff_test[RunInfo],
        cmd_args(ctx.attrs.layer[LayerInfo].parent.subvol_symlink, format = "--parent={}"),
        cmd_args(ctx.attrs.layer[LayerInfo].subvol_symlink, format = "--layer={}"),
    )
    test_cmd = cmd_args(
        base_cmd,
        "test",
        cmd_args(ctx.attrs.diff, format = "--expected={}"),
    )
    return [
        DefaultInfo(sub_targets = {
            "print": [DefaultInfo(), RunInfo(cmd_args(base_cmd, "print"))],
        }),
        RunInfo(test_cmd),
        ExternalRunnerTestInfo(
            command = [test_cmd],
            labels = ctx.attrs.labels,
            type = "custom",
        ),
    ]

_image_diff_test = rule(
    impl = _impl,
    attrs = {
        "diff": attrs.source(doc = "expected diff between the two"),
        "image_diff_test": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/testing/image_diff_test:image-diff-test")),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
    },
    doc = "Test that the only changes between a layer and it's parent is what you expect",
)

def image_diff_test(**kwargs):
    labels = kwargs.pop("labels", [])
    if fully_qualified_test_name_rollout.use_fully_qualified_name():
        labels = labels + [NAMING_ROLLOUT_LABEL]

    _image_diff_test(
        labels = labels,
        **kwargs
    )
