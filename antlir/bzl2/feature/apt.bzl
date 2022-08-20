# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl/image/feature:apt.shape.bzl", "apt_action_item_t")
load("//antlir/bzl2:feature_rule.bzl", "maybe_add_feature_rule")

def feature_apt_install(packages):
    # copy in buck1 version
    return maybe_add_feature_rule(
        name = "apt",
        include_in_target_name = {
            "action": "install",
            "packages": packages,
        },
        feature_shape = apt_action_item_t(
            action = "install",
            package_names = packages,
        ),
    )

def feature_apt_remove_if_exists(packages):
    # copy in buck1 version
    return maybe_add_feature_rule(
        name = "apt",
        include_in_target_name = {
            "action": "remove_if_exists",
            "packages": packages,
        },
        feature_shape = apt_action_item_t(
            action = "remove_if_exists",
            package_names = packages,
        ),
    )
