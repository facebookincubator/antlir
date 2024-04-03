# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/feature:defs.bzl", "feature")
load("//antlir/antlir2/bzl/image:defs.bzl", "image")
load("//antlir/antlir2/testing:image_test.bzl", "image_sh_test")

def _diff_test_impl(ctx: AnalysisContext) -> list[Provider]:
    if not ctx.attrs.layer[LayerInfo].parent:
        fail("image_diff_test only works for layers with parents")

    test_cmd = cmd_args(
        cmd_args("sudo") if (
            not ctx.attrs._rootless and not ctx.attrs.running_in_image
        ) else cmd_args(),
        ctx.attrs.image_diff_test[RunInfo],
        cmd_args("--") if ctx.attrs.running_in_image else cmd_args(),
        cmd_args("--rootless") if ctx.attrs._rootless else cmd_args(),
        cmd_args(ctx.attrs.exclude, format = "--exclude={}"),
        cmd_args(ctx.attrs.diff_type, format = "--diff-type={}"),
        cmd_args(ctx.attrs.diff, format = "--expected={}"),
        cmd_args(
            "/parent" if ctx.attrs.running_in_image else ctx.attrs.layer[LayerInfo].parent[LayerInfo].subvol_symlink,
            format = "--parent={}",
        ),
        cmd_args(
            "/layer" if ctx.attrs.running_in_image else ctx.attrs.layer[LayerInfo].subvol_symlink,
            format = "--layer={}",
        ),
    )

    providers = [
        DefaultInfo(),
        RunInfo(test_cmd),
    ]

    if not ctx.attrs.running_in_image:
        providers.append(ExternalRunnerTestInfo(
            type = "simple",
            command = [test_cmd],
            # FIXME: Consider setting to true
            run_from_project_root = False,
        ))

    return providers

_image_diff_test = rule(
    impl = _diff_test_impl,
    attrs = {
        "diff": attrs.source(doc = "expected diff between the two"),
        "diff_type": attrs.enum(["file", "rpm", "all"], default = "all"),
        "exclude": attrs.list(attrs.string(), default = []),
        "image_diff_test": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/testing/image_diff_test:image-diff-test")),
        "layer": attrs.dep(providers = [LayerInfo]),
        "running_in_image": attrs.bool(),
        "_rootless": rootless_cfg.is_rootless_attr,
    },
    doc = "Test that the only changes between a layer and it's parent is what you expect",
)

def image_diff_test(
        *,
        name: str,
        diff: str | Select,
        layer: str,
        diff_type: str = "all",
        default_os: str | None = None,
        **kwargs):
    needs_rpm = diff_type in ("all", "rpm")

    if needs_rpm:
        _image_diff_test(
            name = name + "--script",
            diff = diff,
            layer = layer,
            diff_type = diff_type,
            running_in_image = True,
            **kwargs
        )

        image.layer(
            name = name + "--test-appliance",
            force_flavor = layer + "[flavor]",
            features = [
                feature.rpms_install(rpms = ["rpm"]),
                feature.layer_mount(
                    source = layer,
                    mountpoint = "/layer",
                ),
                feature.layer_mount(
                    source = layer + "[parent_layer]",
                    mountpoint = "/parent",
                ),
            ],
            default_os = default_os,
        )

        image_sh_test(
            name = name,
            layer = ":{}--test-appliance".format(name),
            test = ":{}--script".format(name),
        )
    else:
        _image_diff_test(
            name = name,
            diff = diff,
            layer = layer,
            diff_type = diff_type,
            running_in_image = False,
            **kwargs
        )
