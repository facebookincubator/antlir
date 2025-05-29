# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")

ApplianceVmInfo = provider(fields = {
    "make_cmd_args": typing.Callable,
})

def _appliance_vm_impl(ctx: AnalysisContext) -> list[Provider]:
    runner = ctx.attrs._runner[RunInfo]
    crosvm = ensure_single_output(ctx.attrs._crosvm)
    default_rootfs = ctx.attrs.default_rootfs
    default_kernel = ctx.attrs.default_kernel

    def make_cmd_args(
            *,
            rootfs: Dependency | None = None,
            kernel: Dependency | None = None,
            timeout_ms: int | None = None) -> cmd_args:
        rootfs = rootfs or default_rootfs
        kernel = kernel or default_kernel
        timeout_ms = timeout_ms or 60000
        return cmd_args(
            runner,
            cmd_args(crosvm, format = "--crosvm={}"),
            cmd_args(ensure_single_output(kernel), format = "--kernel={}"),
            cmd_args(rootfs[LayerInfo].contents.subvol_symlink, format = "--rootfs={}"),
            cmd_args(str(timeout_ms), format = "--timeout-ms={}"),
            "--",
        )

    return [
        DefaultInfo(),
        ApplianceVmInfo(
            make_cmd_args = make_cmd_args,
        ),
    ]

_appliance_vm = rule(
    impl = _appliance_vm_impl,
    attrs = {
        "default_kernel": attrs.dep(),
        "default_rootfs": attrs.dep(providers = [LayerInfo]),
        "_crosvm": attrs.default_only(attrs.dep(default = "antlir//antlir/antlir2/appliance_vm:crosvm")),
        "_runner": attrs.default_only(attrs.dep(default = "antlir//antlir/antlir2/appliance_vm:runner")),
    },
)

appliance_vm = rule_with_default_target_platform(_appliance_vm)
