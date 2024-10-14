# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@fbcode//target_determinator/macros:ci.bzl", "ci")
load("@fbcode//target_determinator/macros:fbcode_ci_helpers.bzl", "fbcode_ci")

def disable_dev_mode(labels: list[str]) -> list[str]:
    """ In addition to `use_opt_instead_of_dev`, explicitly replace some modes
    because it won't catch default dev modes. This function is in its own file
    to make oss-disable happy.
    """
    labels += ci.labels(
        ci.replace({
            ci.linux(ci.x86_64()): ci.linux(ci.x86_64(ci.opt())),
            ci.linux(ci.aarch64()): ci.linux(ci.aarch64(ci.opt())),
        }),
        fbcode_ci.use_opt_instead_of_dev(),
    )
    return labels
