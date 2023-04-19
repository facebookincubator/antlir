# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "ParseTimeFeature")

types.lint_noop()

def _symlink_feature(
        *,
        link: types.or_selector(str.type),
        target: types.or_selector(str.type),
        feature_type: types.or_selector(str.type)) -> ParseTimeFeature.type:
    return ParseTimeFeature(
        feature_type = feature_type,
        kwargs = {
            "is_directory": feature_type == "ensure_dir_symlink",
            "link": link,
            "target": target,
        },
    )

def ensure_file_symlink(*, link: str.type, target: str.type) -> ParseTimeFeature.type:
    """
    Create a symlink to a file.

    Trailing '/'s are not allowed, unlike `ln` where it is significant.
    """
    return _symlink_feature(feature_type = "ensure_file_symlink", link = link, target = target)

def ensure_dir_symlink(*, link: str.type, target: str.type) -> ParseTimeFeature.type:
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
