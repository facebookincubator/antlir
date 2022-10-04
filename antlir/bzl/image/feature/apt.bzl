# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:target_tagger.bzl", "new_target_tagger", "target_tagger_to_feature")
load(":apt.shape.bzl", "apt_action_item_t")

def _build_apt_action(package_list, action):
    apt_action = apt_action_item_t(action = action, package_names = package_list)
    return target_tagger_to_feature(
        new_target_tagger(),
        items = struct(apt = [apt_action]),
    )

def feature_apt_install(package_list):
    return _build_apt_action(package_list, "install")

def feature_apt_remove_if_exists(package_list):
    return _build_apt_action(package_list, "remove_if_exists")
