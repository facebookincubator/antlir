# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @oss-disable
load("@prelude//python:python.bzl", "PythonLibraryInfo")
load("//antlir/bzl:oss_shim.bzl", "rollout", read_bool = "ret_false") # @oss-enable

PYTHON_OUTPLACE_PAR_ROLLOUT = rollout.create_feature(
    {
        # "example_opt_in": True,
        "antlir/antlir2/features/install/tests": True,
        # @oss-disable
        # @oss-disable
        # @oss-disable
    },
)

def is_python_target(target) -> bool:
    return "library-info" in target[DefaultInfo].sub_targets and PythonLibraryInfo in target.sub_target("library-info")
