# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable
# @oss-disable
load("//antlir/antlir2/bzl:platform.bzl", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/testing:image_test.bzl", "HIDE_TEST_LABELS")
load("//antlir/bzl:build_defs.bzl", "add_test_framework_label", "buck_sh_test", "cpp_unittest", "python_unittest", "rust_unittest")
load(":types.bzl", "VMHostInfo")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    test_cmd = cmd_args(
        cmd_args(ctx.attrs.vm_host[VMHostInfo].vm_exec[RunInfo]),
        "test",
        cmd_args(ctx.attrs.vm_host[VMHostInfo].image[LayerInfo].subvol_symlink, format = "--image={}"),
        cmd_args(ctx.attrs.vm_host[VMHostInfo].machine_spec, format = "--machine-spec={}"),
        cmd_args(ctx.attrs.vm_host[VMHostInfo].runtime_spec, format = "--runtime-spec={}"),
        cmd_args(str(ctx.attrs.timeout_s), format = "--timeout-s={}"),
        cmd_args(
            ["{}={}".format(k, v) for k, v in ctx.attrs.test[ExternalRunnerTestInfo].env.items()],
            format = "--setenv={}",
        ),
        ctx.attrs.test[ExternalRunnerTestInfo].test_type,
        ctx.attrs.test[ExternalRunnerTestInfo].command,
    )
    inner_labels = [
        label
        for label in ctx.attrs.test[ExternalRunnerTestInfo].labels
        if label not in HIDE_TEST_LABELS
    ]
    test_script, _ = ctx.actions.write(
        "test.sh",
        cmd_args(
            "#!/bin/bash",
            cmd_args(test_cmd, delimiter = " \\\n  "),
            "\n",
        ),
        is_executable = True,
        allow_args = True,
    )
    return [
        DefaultInfo(test_script),
        ExternalRunnerTestInfo(
            command = [test_cmd],
            type = ctx.attrs.test[ExternalRunnerTestInfo].test_type,
            labels = ctx.attrs.test_labels + inner_labels,
            contacts = ctx.attrs.test[ExternalRunnerTestInfo].contacts,
            env = ctx.attrs.test[ExternalRunnerTestInfo].env,
            run_from_project_root = True,
        ),
    ]

_vm_test = rule(
    impl = _impl,
    attrs = {
        "test": attrs.option(
            attrs.dep(
                providers = [ExternalRunnerTestInfo],
                doc = "Test target to execute inside VM",
            ),
            default = None,
        ),
        "test_labels": attrs.option(
            attrs.list(attrs.string(), default = []),
            default = None,
        ),
        "timeout_s": attrs.int(
            default = 300,
            doc = "total allowed execution time for the test",
        ),
        "vm_host": attrs.dep(providers = [VMHostInfo], doc = "VM host target for the test"),
    },
)

vm_test = rule_with_default_target_platform(_vm_test)

def _implicit_vm_test(
        test_rule,
        name: str,
        vm_host: str,
        labels: [list[str], None] = None,
        **kwargs):
    """Wraps a unit test rule to execute inside a VM. @vm_host must be a VM
    target constructed by `:defs.bzl::vm.host()`."""
    inner_test_name = name + "_vm_test_inner"
    test_rule(
        name = inner_test_name,
        antlir_rule = "user-internal",
        labels = add_test_framework_label(HIDE_TEST_LABELS, "test-framework=8:vmtest"),
        **kwargs
    )

    labels = list(labels) if labels else []
    # @oss-disable
        # @oss-disable

    vm_test(
        name = name,
        test = ":" + inner_test_name,
        test_labels = labels + [special_tags.enable_artifact_reporting],
        vm_host = vm_host,
    )

vm_cpp_test = partial(_implicit_vm_test, cpp_unittest)
vm_python_test = partial(_implicit_vm_test, python_unittest, supports_static_listing = False)
vm_rust_test = partial(_implicit_vm_test, rust_unittest)
vm_sh_test = partial(_implicit_vm_test, buck_sh_test)
