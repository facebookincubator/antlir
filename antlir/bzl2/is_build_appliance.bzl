# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# @lint-ignore-every BUCKRESTRICTEDSYNTAX

load("//antlir/bzl:constants.bzl", "REPO_CFG")

def is_build_appliance(target):
    return target in {
        config.build_appliance: 1
        for _, config in REPO_CFG.flavor_to_config.items()
    }
