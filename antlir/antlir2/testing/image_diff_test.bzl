# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")

def _diff_test_impl(ctx: AnalysisContext) -> list[Provider]:
    if not ctx.attrs.layer[LayerInfo].parent:
        fail("image_diff_test only works for layers with parents")

    test_cmd = cmd_args(
        ctx.attrs.image_diff_test[RunInfo],
        cmd_args("--"),
        cmd_args("/parent", format = "--parent={}"),
        cmd_args("/layer", format = "--layer={}"),
        cmd_args(ctx.attrs.exclude, format = "--exclude={}"),
        cmd_args(ctx.attrs.diff_type, format = "--diff-type={}"),
        cmd_args(ctx.attrs.diff, format = "--expected={}"),
    )

    return [
        DefaultInfo(),
        RunInfo(test_cmd),
    ]

_image_diff_test = rule(
    impl = _diff_test_impl,
    attrs = {
        "diff": attrs.source(doc = "expected diff between the two"),
        "diff_type": attrs.enum(["file", "rpm", "all"], default = "all"),
        "exclude": attrs.list(attrs.string(), default = []),
        "image_diff_test": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/testing/image_diff_test:image-diff-test")),
        "layer": attrs.dep(providers = [LayerInfo]),
    },
    doc = "Test that the only changes between a layer and it's parent is what you expect",
)

def image_diff_test(name: str, diff: str | Select, layer: str, **kwargs):
    _image_diff_test(
        name = name + "--script",
        diff = diff,
        layer = layer,
        **kwargs
    )

    image.layer(
        name = name + "--layer",
        # Diff test does rpm list comparison so we need to run it in the BA where we have
        # `rpm` installed
        parent_layer = layer + "[build_appliance]",
        flavor = layer + "[flavor]",
        features = [
            feature.ensure_dirs_exist(dirs = "/layer"),
            feature.layer_mount(
                source = layer,
                mountpoint = "/layer",
            ),
            feature.ensure_dirs_exist(dirs = "/parent"),
            feature.layer_mount(
                source = layer + "[parent_layer]",
                mountpoint = "/parent",
            ),
        ],
    )

    image_sh_test(
        name = name,
        layer = ":{}--layer".format(name),
        test = ":{}--script".format(name),
    )
