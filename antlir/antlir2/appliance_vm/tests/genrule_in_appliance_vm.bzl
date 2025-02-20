# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/appliance_vm:defs.bzl", "ApplianceVmInfo")
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")

def _genrule_in_appliance_vm_impl(ctx: AnalysisContext) -> list[Provider]:
    out_artifact = ctx.actions.declare_output("out")
    ctx.actions.run(
        cmd_args(
            ctx.attrs.vm[ApplianceVmInfo].make_cmd_args(
                rootfs = ctx.attrs.rootfs,
                kernel = ctx.attrs.kernel,
                timeout_ms = ctx.attrs.timeout_ms,
            ),
            "bash",
            "-c",
            cmd_args(ctx.attrs.bash, quote = "shell"),
        ),
        env = {
            "OUT": out_artifact.as_output(),
        },
        category = "genrule_in_appliance_vm",
        local_only = True,  # needs local subvol
    )
    return [
        DefaultInfo(out_artifact),
    ]

_genrule_in_appliance_vm = rule(
    impl = _genrule_in_appliance_vm_impl,
    # crosvm does not support cross-arch emulation, so everything must be an exec-dep
    attrs = {
        "bash": attrs.arg(),
        "kernel": attrs.option(attrs.exec_dep(), default = None),
        "rootfs": attrs.option(attrs.exec_dep(providers = [LayerInfo]), default = None),
        "timeout_ms": attrs.option(attrs.int(), default = None),
        "vm": attrs.exec_dep(providers = [ApplianceVmInfo], default = "antlir//antlir/antlir2/appliance_vm:appliance_vm"),
    },
)

genrule_in_appliance_vm = rule_with_default_target_platform(_genrule_in_appliance_vm)
