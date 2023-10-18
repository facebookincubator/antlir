# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load(":types.bzl", "VMHostInfo")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    run_cmd = cmd_args(
        cmd_args(ctx.attrs.vm_host[VMHostInfo].vm_exec[RunInfo]),
        "test",
        cmd_args(ctx.attrs.vm_host[VMHostInfo].image[LayerInfo].subvol_symlink, format = "--image={}"),
        cmd_args(ctx.attrs.vm_host[VMHostInfo].machine_spec, format = "--machine-spec={}"),
        cmd_args(ctx.attrs.vm_host[VMHostInfo].runtime_spec, format = "--runtime-spec={}"),
        cmd_args(str(ctx.attrs.timeout_secs), format = "--timeout-secs={}"),
        # (ab)use custom test command to run our random command
        "custom",
        ctx.attrs.command,
    )
    run_script, _ = ctx.actions.write(
        "run_command.sh",
        cmd_args(
            "#!/bin/bash",
            cmd_args(run_cmd, delimiter = " \\\n  "),
            "\n",
        ),
        is_executable = True,
        allow_args = True,
    )
    return [DefaultInfo(run_script), RunInfo(run_cmd)]

_vm_run_command = rule(
    impl = _impl,
    attrs = {
        "command": attrs.arg(doc = "Command to execute inside VM"),
        "timeout_secs": attrs.int(
            default = 300,
            doc = "total allowed execution time for the command",
        ),
        "vm_host": attrs.dep(providers = [VMHostInfo], doc = "VM host target for the test"),
    },
)

vm_run_command = rule_with_default_target_platform(_vm_run_command)
