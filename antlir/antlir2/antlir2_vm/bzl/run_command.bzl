# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load(":types.bzl", "VMHostInfo")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    vm_args_prefix = cmd_args(
        cmd_args(ctx.attrs.vm_host[VMHostInfo].vm_exec[RunInfo]),
        "test",
        cmd_args(ensure_single_output(ctx.attrs.vm_host[VMHostInfo].image), format = "--image={}"),
        cmd_args(ctx.attrs.vm_host[VMHostInfo].machine_spec, format = "--machine-spec={}"),
        cmd_args(str(ctx.attrs.timeout_secs), format = "--timeout-secs={}"),
    )
    vm_args_suffix = cmd_args(
        # (ab)use custom test command to run our random command
        "custom",
        ctx.attrs.command,
    )

    # Forward any args passed by `buck2 run :target -- <args>` as extra
    # antlir2_vm flags before the `custom` command. This lets callers inject
    # e.g. `--console-output-file=/path` to persist the guest serial console
    # to a host-side file -- useful for debugging guest hangs/crashes that
    # produce no stdout from the in-VM command.
    script_cmd = cmd_args(
        vm_args_prefix,
        '"$@"',
        vm_args_suffix,
    )
    run_script, hidden = ctx.actions.write(
        "run_command.sh",
        cmd_args(
            "#!/bin/bash",
            cmd_args(script_cmd, delimiter = " \\\n  "),
            "\n",
        ),
        # Absolute artifact paths so the script works regardless of cwd
        # (skycastle runs it with cwd=fbcode; buck-out lives at fbsource root).
        absolute = True,
        is_executable = True,
        allow_args = True,
        # Materialize embedded artifacts when invoked via `buck2 run`.
        with_inputs = True,
        has_content_based_path = False,
    )
    # Run the script (not the inner cmd_args) so `buck2 run :target -- <args>`
    # gets the same `"$@"` forwarding as direct script invocation. `hidden`
    # threads the script's input artifacts through RunInfo for materialization.
    return [DefaultInfo(run_script), RunInfo(args = cmd_args(run_script, hidden = hidden))]

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
