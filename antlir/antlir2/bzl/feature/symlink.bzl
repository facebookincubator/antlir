# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn")

_STR_OR_SELECTOR = types.or_selector(str.type)

types.lint_noop(_STR_OR_SELECTOR)

def _symlink_feature(
        *,
        link: _STR_OR_SELECTOR,
        target: _STR_OR_SELECTOR,
        feature_type: _STR_OR_SELECTOR) -> ParseTimeFeature.type:
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

symlink_analyze = data_only_feature_analysis_fn(symlink_record)
