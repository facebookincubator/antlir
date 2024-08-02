# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/antlir2_rootless:cfg.bzl", "rootless_cfg")
load("//antlir/antlir2/antlir2_rootless:package.bzl", "get_antlir2_rootless")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/bzl/image:cfg.bzl", "cfg_attrs", "layer_cfg")
load("//antlir/antlir2/os:package.bzl", "get_default_os_for_package")

def _diff_test_impl(ctx: AnalysisContext) -> list[Provider]:
    if not ctx.attrs.layer[LayerInfo].parent:
        fail("image_diff_test only works for layers with parents")

    test_cmd = cmd_args(
        cmd_args("sudo") if not ctx.attrs._rootless else cmd_args(),
        ctx.attrs.image_diff_test[RunInfo],
        cmd_args("--rootless") if ctx.attrs._rootless else cmd_args(),
        cmd_args(ctx.attrs.exclude, format = "--exclude={}"),
        cmd_args(ctx.attrs.diff_type, format = "--diff-type={}"),
        cmd_args(ctx.attrs.diff, format = "--expected={}"),
        cmd_args(
            ctx.attrs.layer[LayerInfo].parent[LayerInfo].subvol_symlink,
            format = "--parent={}",
        ),
        cmd_args(
            ctx.attrs.layer[LayerInfo].parent[LayerInfo].facts_db,
            format = "--parent-facts-db={}",
        ),
        cmd_args(
            ctx.attrs.layer[LayerInfo].subvol_symlink,
            format = "--layer={}",
        ),
        cmd_args(
            ctx.attrs.layer[LayerInfo].facts_db,
            format = "--facts-db={}",
        ),
    )

    return [
        DefaultInfo(),
        RunInfo(test_cmd),
        ExternalRunnerTestInfo(
            type = "simple",
            command = [test_cmd],
        ),
    ]

_image_diff_test = rule(
    impl = _diff_test_impl,
    attrs = {
        "diff": attrs.source(doc = "expected diff between the two"),
        "diff_type": attrs.enum(["file", "rpm", "all"], default = "all"),
        "exclude": attrs.list(attrs.string(), default = []),
        "image_diff_test": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/testing/image_diff_test:image-diff-test")),
        "labels": attrs.list(attrs.string(), default = []),
        "layer": attrs.dep(providers = [LayerInfo]),
        "_rootless": rootless_cfg.is_rootless_attr,
    } | cfg_attrs(),
    doc = "Test that the only changes between a layer and it's parent is what you expect",
    cfg = layer_cfg,
)

_image_diff_test_macro = rule_with_default_target_platform(_image_diff_test)

def image_diff_test(
        *,
        name: str,
        default_os: str | None = None,
        rootless: bool | None = None,
        **kwargs):
    rootless = rootless if rootless != None else get_antlir2_rootless()
    labels = kwargs.pop("labels", [])
    labels = list(labels)
    if not rootless:
        labels.append("uses_sudo")

    _image_diff_test_macro(
        name = name,
        default_os = default_os or get_default_os_for_package(),
        rootless = rootless,
        labels = labels,
        **kwargs
    )
