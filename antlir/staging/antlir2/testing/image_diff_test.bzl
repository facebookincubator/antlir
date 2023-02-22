# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/staging/antlir2:antlir2_layer.bzl", "LayerInfo")

def _impl(ctx: "context") -> ["provider"]:
    if not ctx.attrs.layer[LayerInfo].parent:
        fail("image_diff_test only works for layers with parents")
    base_cmd = cmd_args(
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
            type = "custom",
        ),
    ]

image_diff_test = rule(
    impl = _impl,
    attrs = {
        "diff": attrs.source(doc = "expected diff between the two"),
        "image_diff_test": attrs.default_only(attrs.exec_dep(default = "//antlir/staging/antlir2/testing/image_diff_test:image-diff-test")),
        "layer": attrs.dep(providers = [LayerInfo]),
    },
    doc = "Test that the only changes between a layer and it's parent is what you expect",
)
