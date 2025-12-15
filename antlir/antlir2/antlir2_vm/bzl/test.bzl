# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("@fbcode_macros//build_defs:fully_qualified_test_name_rollout.bzl", "NAMING_ROLLOUT_LABEL", "fully_qualified_test_name_rollout")
# @oss-disable[end= ]: load("@fbsource//tools/build_defs:testpilot_defs.bzl", "special_tags")
# @oss-disable[end= ]: load("@fbsource//tools/target_determinator/macros:ci.bzl", "ci")
load("@prelude//utils:selects.bzl", "selects")
load("//antlir/antlir2/bzl:platform.bzl", "arch_select", "rule_with_default_target_platform")
load("//antlir/antlir2/testing:image_test.bzl", "HIDE_TEST_LABELS", "env_from_wrapped_test")
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")
load("//antlir/bzl:build_defs.bzl", "add_test_framework_label", "buck_sh_test", "cpp_unittest", "python_unittest", "rust_unittest")
# @oss-disable[end= ]: load(":disable_dev_mode.bzl", "disable_dev_mode")

load("//antlir/bzl:oss_shim.bzl", "NAMING_ROLLOUT_LABEL", "special_tags", "fully_qualified_test_name_rollout") # @oss-enable
load(":types.bzl", "VMHostInfo")

def _impl(ctx: AnalysisContext) -> list[Provider]:
    inner_labels = [
        label
        for label in ctx.attrs.test[ExternalRunnerTestInfo].labels
        if label not in HIDE_TEST_LABELS
    ]

    # Extend tpx timeout to 100 minutes if we exceed the default 10 min plus buffer
    # @oss-disable[end= ]: if ctx.attrs.timeout_secs + 60 > 600:
        # @oss-disable[end= ]: inner_labels.append("long_running")

    common_args = cmd_args(
        cmd_args(ensure_single_output(ctx.attrs.vm_host[VMHostInfo].image), format = "--image={}"),
        cmd_args(ctx.attrs.vm_host[VMHostInfo].machine_spec, format = "--machine-spec={}"),
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
    if ctx.attrs.dump_eth0:
        test_cmd = cmd_args(test_cmd, "--dump-eth0-traffic")

    if ctx.attrs.output_dirs:
        for output_dir in ctx.attrs.output_dirs:
            test_cmd = cmd_args(test_cmd, "--output-dirs", output_dir)

    if ctx.attrs.systemd_credentials:
        credentials = ["{}={}".format(k, v) for k, v in ctx.attrs.systemd_credentials.items()]
        test_cmd = cmd_args(test_cmd, cmd_args(credentials, format = "--systemd-credential={}"))

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

    env = env_from_wrapped_test(ctx.attrs.test)
    if ctx.attrs._static_list_wrapper:
        original = env.pop("STATIC_LIST_TESTS_BINARY", None)
        if original:
            env["STATIC_LIST_TESTS_BINARY"] = RunInfo(cmd_args(
                ctx.attrs._static_list_wrapper[RunInfo],
                cmd_args(original, format = "--wrap={}"),
            ))

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
            env = env,
            use_project_relative_paths = ctx.attrs.test[ExternalRunnerTestInfo].use_project_relative_paths,
            run_from_project_root = ctx.attrs.test[ExternalRunnerTestInfo].run_from_project_root,
            default_executor = CommandExecutorConfig(
                local_enabled = True,
                # We don't (yet) have an RE platform where we can access KVM and
                # create TAP network devices
                remote_enabled = False,
            ),
        ),
    ]

_vm_test = rule(
    impl = _impl,
    attrs = {
        "dump_eth0": attrs.bool(
            doc = "If true, dumps the vm's eth0 traffic to a file. The file location is dictated by testX and uploaded as part of test result",
            default = bool(read_config("antlir2", "dump_eth0", False)),
        ),
        "expect_failure": attrs.bool(
            doc = "If true, VM is expected to timeout or fail early.",
        ),
        "first_boot_command": attrs.option(
            attrs.arg(doc = "Command to execute on first boot. The test \
            will be executed at the second boot."),
            default = None,
        ),
        "labels": attrs.list(attrs.string(), default = []),
        "output_dirs": attrs.option(
            attrs.list(attrs.arg(doc = "Output directories to mount into the VM. This should be a macro, like $(location //path/to:some_target)")),
            default = None,
        ),
        "postmortem": attrs.bool(
            doc = "If true, the test is run after VM is terminated and its console log is accessible \
            through env $CONSOLE_OUTPUT. This is usually combined with @expect_failure to validate \
            failure scenarios.",
        ),
        "systemd_credentials": attrs.dict(
            attrs.string(),
            # There's no reason why these can't also include passing source
            # files / build targets, but for now we only really need static
            # strings, so don't make it overcomplicated
            attrs.string(),
            doc = "credential kv pairs to pass to systemd",
            default = {},
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
        "_static_list_wrapper": attrs.option(attrs.exec_dep(), default = None),
    },
)

vm_test = rule_with_default_target_platform(_vm_test, local_only_exec = True)

def _get_internal_labels(test_rule, run_as_bundle: bool) -> (list[str], list[str], list[str]):
    """ Returns a set of labels (inner_labels, wrapper_labels, ci_labels)
    inner_labels are for the inner test target we wrap with vm
    wrapper_labels are test labels for the vmtest target
    ci_labels are CI labels for the vmtest target
    """
    wrapper_labels = ["heavyweight"]
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
    # @oss-disable[end= ]: inner_labels += [ci.skip_target()]

    # don't run dev mode for vmtests
    ci_labels = []
    # @oss-disable[end= ]: ci_labels += disable_dev_mode(wrapper_labels)

    ## "tpx:supports_coverage" label is required to Citadel to discover
    ## test that produce coverage
    if test_rule == python_unittest or test_rule == cpp_unittest:
        ci_labels += ["tpx:supports_coverage"]

    return inner_labels, wrapper_labels, ci_labels

def _implicit_vm_test(
        test_rule,
        *,
        name: str,
        vm_host: str,
        run_as_bundle: bool = False,
        timeout_secs: None | int | Select = None,
        first_boot_command: None | str = None,
        expect_failure: bool = False,
        postmortem: bool = False,
        labels: list[str] | None = None,
        # @oss-disable[end= ]: vm_test_labels: list[str] | None = None,
        output_dirs: list[str] | None = None,
        systemd_credentials: dict[str, str] | None = None,
        _add_outer_labels: list[str] = [],
        _static_list_wrapper: str | None = None,
        **kwargs) -> list[str]:
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

    # try to apply a timeout multiplier if specified on the command line
    timeout_multiplier = int(read_config("antlir2", "timeout_multiplier", 1))
    timeout_secs = selects.apply(timeout_secs, lambda x: x * timeout_multiplier)

    wrapper_labels = list(labels) if labels else []
    wrapper_labels.extend(_add_outer_labels)
    inner_labels = []

    # @oss-disable[end= ]: inner, wrapper, ci_labels = _get_internal_labels(test_rule, run_as_bundle)
    # @oss-disable[end= ]: wrapper_labels.extend(wrapper)
    # @oss-disable[end= ]: inner_labels.extend(inner)
    labels = ["uses_sudo"]
    # @oss-disable[end= ]: labels += ci_labels
    # @oss-disable[end= ]: labels += vm_test_labels or []

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
        output_dirs = output_dirs,
        systemd_credentials = systemd_credentials or {},
        compatible_with = kwargs.get("compatible_with"),
        _static_list_wrapper = _static_list_wrapper,
        target_compatible_with = kwargs.get("target_compatible_with"),
        labels = labels,
    )
    return [":" + name]

vm_cpp_test = partial(
    _implicit_vm_test,
    cpp_unittest,
    _static_list_wrapper = "antlir//antlir/antlir2/antlir2_vm:static-list-cpp",
    # @oss-disable[end= ]: _add_outer_labels = ["tpx:optout-test-result-output-spec", "tpx:supports_coverage"],
)
vm_python_test = partial(
    _implicit_vm_test,
    python_unittest,
    _static_list_wrapper = "antlir//antlir/antlir2/antlir2_vm:static-list-py",
    _add_outer_labels = ["tpx:supports_coverage"],
)
vm_rust_test = partial(_implicit_vm_test, rust_unittest)
vm_sh_test = partial(_implicit_vm_test, buck_sh_test)
