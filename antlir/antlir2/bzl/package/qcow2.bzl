# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@antlir//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load(":disk_image.bzl", "DiskImage")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    out = ctx.actions.declare_output("package.qcow2")
    ctx.actions.run(
        cmd_args(
            ctx.attrs._qemu_img[RunInfo],
            "convert",
            "-f",
            "raw",
            "-O",
            "qcow2",
            ctx.attrs.src[DiskImage].src,
            out.as_output(),
        ),
        category = "qemu_img",
    )

    return [DefaultInfo(out, sub_targets = {"src": ctx.attrs.src.providers})]

qcow2_rule = anon_rule(
    impl = _impl,
    attrs = {
        "src": attrs.dep(providers = [DiskImage]),
        "_qemu_img": attrs.default_only(
            attrs.exec_dep(
                default = "antlir//antlir/antlir2/bzl/package:qemu-img",
            ),
        ),
    },
    artifact_promise_mappings = {
        "qcow2": lambda x: ensure_single_output(x),
    },
)

qcow2 = rule_with_default_target_platform(qcow2_rule)
