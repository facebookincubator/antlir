# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

def _symlink_feature(
        *,
        link: str.type,
        target: str.type,
        feature_type: str.type) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = feature_type,
        kwargs = {
            "is_directory": feature_type == "ensure_dir_symlink",
            "link": link,
            "target": target,
        },
    )

ensure_file_symlink = partial(_symlink_feature, feature_type = "ensure_file_symlink")
ensure_dir_symlink = partial(_symlink_feature, feature_type = "ensure_dir_symlink")

def symlink_to_json(
        link: str.type,
        target: str.type,
        is_directory: bool.type) -> {str.type: ""}:
    return {
        "is_directory": is_directory,
        "link": link,
        "target": target,
    }
