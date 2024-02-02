# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:new_sets.bzl", "sets")
load("//antlir/antlir2/testing:image_rpms_test.bzl?v2_only", antlir2_rpm_names_test = "image_test_rpm_names")
load(":antlir2_shim.bzl", "antlir2_shim")

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
        rpm_list,
        flavor = None,
        antlir2 = None):
    if antlir2_shim.upgrade_or_shadow_test(
        antlir2 = antlir2,
        fn = antlir2_rpm_names_test,
        name = name,
        src = rpm_list,
        layer = layer,
    ) != "upgrade":
        fail("antlir1 is dead")
