# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:constants.bzl", "REPO_CFG")
load(":flavor_impl.bzl", "flavor_to_struct")

def check_flavor_exists(flavor):
    flavor = flavor_to_struct(flavor)
    if flavor.name not in REPO_CFG.flavor_to_config:
        fail(
            "{} must be in {}"
                .format(flavor.name, list(REPO_CFG.flavor_to_config)),
            "flavor",
        )
