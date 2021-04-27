# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")

def check_flavor_exists(flavor):
    if flavor not in REPO_CFG.flavor_to_config:
        fail(
            "{} must be in {}"
                .format(flavor, list(REPO_CFG.flavor_to_config)),
            "flavor",
        )
