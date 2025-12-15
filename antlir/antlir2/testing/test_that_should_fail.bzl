# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable[end= ]: load("@fbcode_macros//build_defs:fully_qualified_test_name_rollout.bzl", "NAMING_ROLLOUT_LABEL", "fully_qualified_test_name_rollout")
# @oss-disable[end= ]: load("@fbsource//tools/build_defs:testpilot_defs.bzl", "special_tags")
load("//antlir/antlir2/bzl:platform.bzl", "default_target_platform_kwargs")
load("//antlir/bzl:build_defs.bzl", "buck_sh_test", "cpp_unittest", "python_unittest", "rust_unittest")
load("//antlir/bzl:oss_shim.bzl", "special_tags") # @oss-enable

_HIDE_TEST_LABELS = [special_tags.disabled, special_tags.test_is_invisible_to_testpilot]

def _impl(ctx: AnalysisContext) -> list[Provider]:
    test_cmd = cmd_args(
        ctx.attrs.test_that_should_fail[RunInfo],
        cmd_args(ctx.attrs.stdout_re, format = "--stdout-re={}") if ctx.attrs.stdout_re else cmd_args(),
        cmd_args(ctx.attrs.stderr_re, format = "--stderr-re={}") if ctx.attrs.stderr_re else cmd_args(),
        "--",
        ctx.attrs.test[ExternalRunnerTestInfo].command,
    )

    # Copy the labels from the inner test since there is tons of behavior
    # controlled by labels and we don't want to have to duplicate logic that
    # other people are already writing in the standard *_unittest macros.
    # This wrapper should be as invisible as possible.
    inner_labels = list(ctx.attrs.test[ExternalRunnerTestInfo].labels)
    for label in _HIDE_TEST_LABELS:
        inner_labels.remove(label)

    script, _ = ctx.actions.write(
        "test.sh",
        cmd_args("#!/bin/bash", cmd_args(test_cmd, delimiter = " \\\n  ")),
        is_executable = True,
        allow_args = True,
    )
    return [
        ExternalRunnerTestInfo(
            command = [test_cmd],
            type = "custom",
            labels = ctx.attrs.labels + inner_labels,
            contacts = ctx.attrs.test[ExternalRunnerTestInfo].contacts,
            env = ctx.attrs.test[ExternalRunnerTestInfo].env,
            run_from_project_root = ctx.attrs.test[ExternalRunnerTestInfo].run_from_project_root,
            default_executor = ctx.attrs.test[ExternalRunnerTestInfo].default_executor,
        ),
        RunInfo(test_cmd),
        DefaultInfo(script),
    ]

_test_that_should_fail = rule(
    impl = _impl,
    attrs = {
        "labels": attrs.list(attrs.string(), default = []),
        "stderr_re": attrs.option(attrs.string(doc = "regex to match on test output"), default = None),
        "stdout_re": attrs.option(attrs.string(doc = "regex to match on test output"), default = None),
        "test": attrs.dep(providers = [ExternalRunnerTestInfo]),
        "test_that_should_fail": attrs.default_only(attrs.exec_dep(default = "//antlir/antlir2/testing/test_that_should_fail:test-that-should-fail")),
    },
    doc = "Run another test that is supposed to fail and ensure it fails for the reason expected",
)

# Collection of helpers to create the inner test implicitly, and hide it from
# TestPilot

def test_that_should_fail(
        test_rule,
        name: str,
        stdout_re: str | None = None,
        stderr_re: str | None = None,
        labels: list[str] | None = None,
        **kwargs):
    test_rule(
        name = name + "_failing_inner_test",
        labels = _HIDE_TEST_LABELS,
        **kwargs
    )
    labels = list(labels) if labels else []

    # @oss-disable[end= ]: if fully_qualified_test_name_rollout.use_fully_qualified_name():
        # @oss-disable[end= ]: labels = labels + [NAMING_ROLLOUT_LABEL]

    _test_that_should_fail(
        name = name,
        test = ":" + name + "_failing_inner_test",
        labels = labels,
        stdout_re = stdout_re,
        stderr_re = stderr_re,
        # Test execution platform is not *usually* where tests run, but since
        # `image_diff_test` is `local_only=True`, use this to force exec_deps to
        # resolve to the host platform where the test is actually going to
        # execute
        exec_compatible_with = ["prelude//platforms:may_run_local"],
        **default_target_platform_kwargs()
    )

cpp_test_that_should_fail = partial(
    test_that_should_fail,
    cpp_unittest,
)
python_test_that_should_fail = partial(
    test_that_should_fail,
    python_unittest,
)
rust_test_that_should_fail = partial(
    test_that_should_fail,
    rust_unittest,
)
sh_test_that_should_fail = partial(
    test_that_should_fail,
    buck_sh_test,
)
