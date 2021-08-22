# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load(":constants.bzl", "REPO_CFG")
load(":image_python_unittest.bzl", "image_python_unittest")
load(":maybe_export_file.bzl", "maybe_export_file")
load(":oss_shim.bzl", "buck_genrule")

def _str_set(s):
    return sets.make([s[i] for i in range(len(s))])

_VALID_PYTHON_IDENTIFIER = _str_set(
    "_0123456789abcdefghijklmnopqrstuvwxyzABCDEFGHIJKLMNOPQRSTUVWXYZ",
)

# If you have a layer where overall size or exact content is a concern, it
# can be meaningful to have CI fail when the set of the installed RPM names
# changes.  This ensures that no change happens by accident.  This test
# harness makes it easy to validate this.
#
# IMPORTANT: `name` must be a valid Python identifier starting with `test_`.
#
# To populate the initial RPM list, just commit this to source control in
# the directory that invokes the `image_test_rpm_names` rule:
#   buck run :layer=container -- -- rpm -qa --queryformat '%{NAME}\n' |
#     sort > rpm_list
def image_test_rpm_names(
        name,
        layer,
        rpm_list):
    fn_name = name  # Future: if we must allow dashes, replace them here.
    if not fn_name.startswith("test_") or not sets.is_subset(
        _str_set(fn_name),
        _VALID_PYTHON_IDENTIFIER,
    ):
        fail(
            "Must be a valid Python identifier starting 'with `test_`",
            "name",
        )

    py_name = fn_name + ".py"
    buck_genrule(
        name = py_name,
        out = py_name,
        bash = """\
cat > "$OUT" <<'A Hilariously Unlikely Yet Cheeky Sigil'
import unittest

from antlir.bzl.tests.check_rpm_names import check_rpm_names

class TestRpmNames(unittest.TestCase):
    def {fn_name}(self):
        check_rpm_names(self, __package__, 'expected_rpm_names')
A Hilariously Unlikely Yet Cheeky Sigil
""".format(fn_name = fn_name),
        antlir_rule = "user-internal",
    )

    image_python_unittest(
        name = name,
        layer = layer,
        srcs = {":" + py_name: py_name},
        resources = {maybe_export_file(rpm_list): "expected_rpm_names"},
        deps = ["//antlir/bzl/tests:check_rpm_names"],
    )
