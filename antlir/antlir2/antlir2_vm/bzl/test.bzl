# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable
# @oss-disable
# @oss-disable
# @oss-disable
load("//antlir/antlir2/bzl:platform.bzl", "arch_select", "rule_with_default_target_platform")
load("//antlir/antlir2/bzl:types.bzl", "LayerInfo")
load("//antlir/antlir2/testing:image_test.bzl", "HIDE_TEST_LABELS")
load("//antlir/bzl:build_defs.bzl", "add_test_framework_label", "buck_sh_test", "cpp_unittest", "python_unittest", "rust_unittest")

load("//antlir/bzl:oss_shim.bzl", "NAMING_ROLLOUT_LABEL", "special_tags", "fully_qualified_test_name_rollout") # @oss-enable
load(":types.bzl", "VMHostInfo")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    inner_labels = [
        label
        for label in ctx.attrs.test[ExternalRunnerTestInfo].labels
        if label not in HIDE_TEST_LABELS
    ]

    # Extend tpx timeout to 100 minutes if we exceed the default 10 min plus buffer
    # @oss-disable
        # @oss-disable

    common_args = cmd_args(
        cmd_args(ctx.attrs.vm_host[VMHostInfo].image[LayerInfo].subvol_symlink, format = "--image={}"),
        cmd_args(ctx.attrs.vm_host[VMHostInfo].machine_spec, format = "--machine-spec={}"),
        cmd_args(ctx.attrs.vm_host[VMHostInfo].runtime_spec, format = "--runtime-spec={}"),
        cmd_args([k for k in ctx.attrs.test[ExternalRunnerTestInfo].env], format = "--passenv={}"),
    )

    test_cmd = cmd_args(
        cmd_args(ctx.attrs.vm_host[VMHostInfo].vm_exec[RunInfo]),
        "test",
        common_args,
        cmd_args(str(ctx.attrs.timeout_secs), format = "--timeout-secs={}"),
    )
    if ctx.attrs.first_boot_command:
        test_cmd = cmd_args(test_cmd, cmd_args(ctx.attrs.first_boot_command, format = "--first-boot-command={}"))
    if ctx.attrs.expect_failure:
        test_cmd = cmd_args(test_cmd, "--expect-failure")
    if ctx.attrs.postmortem:
        test_cmd = cmd_args(test_cmd, "--postmortem")
    test_cmd = cmd_args(
        test_cmd,
        ctx.attrs.test[ExternalRunnerTestInfo].test_type,
        ctx.attrs.test[ExternalRunnerTestInfo].command,
    )

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

    # vm_exec will spawn a shell inside VM
    shell_cmd = cmd_args(
        cmd_args(ctx.attrs.vm_host[VMHostInfo].vm_exec[RunInfo]),
        "isolate",
        common_args,
    )

    # Show console output and drop to console prompt. This is intended for
    # initrd tests that don't boot an OS.
    console_cmd = cmd_args(
        cmd_args(ctx.attrs.vm_host[VMHostInfo].vm_exec[RunInfo]),
        "isolate",
        "--console",
        common_args,
    )

    # Drop to container shell outside VM. This is intended for debugging VM
    # setup. It's the same as `:vm_host[container]`.
    container_cmd = cmd_args(
        cmd_args(ctx.attrs.vm_host[VMHostInfo].vm_exec[RunInfo]),
        "isolate",
        "--container",
        common_args,
    )

    return [
        DefaultInfo(
            test_script,
            sub_targets = {
                "console": [DefaultInfo(test_script), RunInfo(console_cmd)],
                "container": [DefaultInfo(test_script), RunInfo(container_cmd)],
                "inner_test": ctx.attrs.test.providers,
                "shell": [DefaultInfo(test_script), RunInfo(shell_cmd)],
            },
        ),
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
        "expect_failure": attrs.bool(
            doc = "If true, VM is expected to timeout or fail early.",
        ),
        "first_boot_command": attrs.option(
            attrs.arg(doc = "Command to execute on first boot. The test \
            will be executed at the second boot."),
            default = None,
        ),
        "postmortem": attrs.bool(
            doc = "If true, the test is run after VM is terminated and its console log is accessible \
            through env $CONSOLE_OUTPUT. This is usually combined with @expect_failure to validate \
            failure scenarios.",
        ),
        "test": attrs.dep(
            providers = [ExternalRunnerTestInfo],
            doc = "Test target to execute. It's executed inside the VM unless @postmortem is set.",
        ),
        "test_labels": attrs.option(
            attrs.list(attrs.string(), default = []),
            default = None,
        ),
        "timeout_secs": attrs.int(doc = "total allowed execution time for the test"),
        "vm_host": attrs.dep(providers = [VMHostInfo], doc = "VM host target for the test"),
    },
)

vm_test = rule_with_default_target_platform(_vm_test)

def _get_internal_labels(test_rule, run_as_bundle: bool):
    wrapper_labels = ["heavyweight"]
    # @oss-disable
    if run_as_bundle:
        wrapper_labels.append(special_tags.run_as_bundle)
    if fully_qualified_test_name_rollout.use_fully_qualified_name():
        wrapper_labels = wrapper_labels + [NAMING_ROLLOUT_LABEL]
    wrapper_labels.append(special_tags.enable_artifact_reporting)

    inner_labels = add_test_framework_label(HIDE_TEST_LABELS, "test-framework=8:vmtest")

    # Due to a complex internal migration, these labels are required to both
    # change the runtime behavior of the outer test, as well as build-time
    # behavior of the inner target.
    if test_rule == python_unittest:
        wrapper_labels.append("use-testpilot-adapter")
        inner_labels.append("use-testpilot-adapter")

        # this tag gets added to the inner test automatically, but we must
        # inform tpx that the wrapper observes the same behavior
        wrapper_labels.append("tpx:list-format-migration:json")

        # also annotate wrapper target with a framework
        wrapper_labels = add_test_framework_label(wrapper_labels, "test-framework=8:vmtest")

    # never schedule any CI on this inner target
    # @oss-disable

    return inner_labels, wrapper_labels

def _implicit_vm_test(
        test_rule,
        name: str,
        vm_host: str,
        run_as_bundle: bool = False,
        timeout_secs: None | int | Select = None,
        first_boot_command: None | str = None,
        expect_failure: bool = False,
        postmortem: bool = False,
        labels: list[str] | None = None,
        _add_outer_labels: list[str] = [],
        **kwargs):
    """Wraps a unit test rule to execute inside a VM. @vm_host must be a VM
    target constructed by `:defs.bzl::vm.host()`.

    @run_as_bundle
        Provide a mechanism for users to control running all the test cases
        defined in a single unittest as a bundle.  Running as a bundle means
        that only *one* VM instance will be spun up for the whole unittest
        and all test cases will be executed inside that single VM instance.
        This might have undesirable effects if the test case is intentionally
        doing something that changes the state of the VM that cannot or
        should not be undone by the test fixture (ie, rebooting or setting
        a sysctl that cannot be undone for example).
    """

    # We only execute aarch64 tests on x64 hosts for now and cross-platform
    # emulation is slower. Give more buffer based on additional boot time.
    timeout_secs = timeout_secs or arch_select(x86_64 = 300, aarch64 = 600)
    wrapper_labels = list(labels) if labels else []
    wrapper_labels.extend(_add_outer_labels)
    inner_labels = []

    # @oss-disable
    # @oss-disable
    # @oss-disable

    inner_test_name = name + "_vm_test_inner"
    test_rule(
        name = inner_test_name,
        labels = inner_labels,
        **kwargs
    )

    vm_test(
        name = name,
        test = ":" + inner_test_name,
        test_labels = wrapper_labels,
        vm_host = vm_host,
        timeout_secs = timeout_secs,
        first_boot_command = first_boot_command,
        expect_failure = expect_failure,
        postmortem = postmortem,
        compatible_with = kwargs.get("compatible_with"),
        target_compatible_with = kwargs.get("target_compatible_with"),
    )

vm_cpp_test = partial(
    _implicit_vm_test,
    cpp_unittest,
    # @oss-disable
    supports_static_listing = False,
)
vm_python_test = partial(_implicit_vm_test, python_unittest, supports_static_listing = False)
vm_rust_test = partial(_implicit_vm_test, rust_unittest)
vm_sh_test = partial(_implicit_vm_test, buck_sh_test)
