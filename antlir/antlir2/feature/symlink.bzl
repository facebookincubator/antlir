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

def ensure_file_symlink(*, link: str.type, target: str.type) -> InlineFeatureInfo.type:
    """
    Create a symlink to a file.

    Trailing '/'s are not allowed, unlike `ln` where it is significant.
    """
    return _symlink_feature(feature_type = "ensure_file_symlink", link = link, target = target)

def ensure_dir_symlink(*, link: str.type, target: str.type) -> InlineFeatureInfo.type:
    """
    Create a symlink to a directory.

    Trailing '/'s are not allowed, unlike `ln` where it is significant.
    """
    return _symlink_feature(feature_type = "ensure_dir_symlink", link = link, target = target)

symlink_record = record(
    link = str.type,
    target = str.type,
    is_directory = bool.type,
)

symlink_to_json = symlink_record
