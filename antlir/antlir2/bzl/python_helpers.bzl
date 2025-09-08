# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@fbsource//tools/build_defs:feature_rollout_utils.bzl", "rollout")
load("@prelude//python:python.bzl", "PythonLibraryInfo")

PYTHON_OUTPLACE_PAR_ROLLOUT = rollout.create_feature(
    {
        # "example_opt_in": True,
        "antlir/antlir2/features/install/tests": True,
        "fblite/devx/fixmydevenv": True,
        "python/pylot": True,
        "registry/builder/rpm/bzl/mac_sign/tests": True,
    },
)

def is_python_target(target) -> bool:
    return "library-info" in target[DefaultInfo].sub_targets and PythonLibraryInfo in target.sub_target("library-info")
