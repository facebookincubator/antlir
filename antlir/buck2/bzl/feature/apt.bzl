# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

def apt_install(*, packages: [str.type]) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "apt_install",
        kwargs = {
            "action": "install",
            "packages": packages,
        },
    )

def apt_remove_if_exists(*, packages: [str.type]) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "apt_remove_if_exists",
        kwargs = {
            "action": "remove_if_exists",
            "packages": packages,
        },
    )

def apt_to_json(
        action: str.type,
        packages: [str.type]) -> {str.type: ""}:
    return {
        "action": action,
        "packages": packages,
    }
