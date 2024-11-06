# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@fbcode_macros//build_defs:native_rules.bzl", "buck_sh_test", _test_suite = "test_suite")

def _test_aggregate_packages_coverage(
        *,
        name: str,
        package_tests: list[str],
        package_includes: list[str]):
    args = ["test_aggregate_packages_coverage"]
    for i in package_tests:
        args += ["--package-test", i]
    for i in package_includes:
        args += ["--package-include", i]
    buck_sh_test(
        name = name + "-test-aggregate-package-tests-coverage",
        test = "antlir//antlir/bzl:test_suite.sh",
        args = args,
        labels = ["local_only"],  # Force local execution.
        exec_compatible_with = ["prelude//platforms:runs_only_local"],
    )

def _aggregate_package_tests(
        *,
        name: str,
        package_tests: list[str],
        package_includes: list[str],
        **kwargs):
    package_tests = [i + ":package-tests" for i in package_tests]
    _test_aggregate_packages_coverage(name = name, package_tests = package_tests, package_includes = package_includes)
    _test_suite(name = name, tests = package_tests, **kwargs)

def _test_package_tests_converage(
        *,
        tests: list[str]):
    args = ["test_package_tests_coverage", "--package", native.package_name()]
    for i in tests:
        args += ["--test", i]
    buck_sh_test(
        name = "test-package-tests-coverage",
        test = "@antlir//antlir/bzl:test_suite.sh",
        args = args,
        labels = ["local_only"],  # Force local execution.
        exec_compatible_with = ["prelude//platforms:runs_only_local"],
    )

def _package_tests(
        *,
        tests: list[str],
        **kwargs):
    _test_package_tests_converage(tests = tests)
    _test_suite(name = "package-tests", tests = tests, **kwargs)

test_suite = struct(
    package_tests = _package_tests,
    aggregate_package_tests = _aggregate_package_tests,
)
