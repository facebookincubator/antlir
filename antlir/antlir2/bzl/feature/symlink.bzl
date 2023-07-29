# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn")

def _symlink_feature(
        *,
        link: str | "selector",
        target: str | "selector",
        feature_type: str | "selector") -> ParseTimeFeature.type:
    return ParseTimeFeature(
        feature_type = feature_type,
        impl = "//antlir/antlir2/features:symlink",
        kwargs = {
            "is_directory": feature_type == "ensure_dir_symlink",
            "link": link,
            "target": target,
        },
    )

def ensure_file_symlink(*, link: str, target: str) -> ParseTimeFeature.type:
    """
    Create a symlink to a file.

    Trailing '/'s are not allowed, unlike `ln` where it is significant.
    """
    return _symlink_feature(feature_type = "ensure_file_symlink", link = link, target = target)

def ensure_dir_symlink(*, link: str, target: str) -> ParseTimeFeature.type:
    """
    Create a symlink to a directory.

    Trailing '/'s are not allowed, unlike `ln` where it is significant.
    """
    return _symlink_feature(feature_type = "ensure_dir_symlink", link = link, target = target)

symlink_record = record(
    link = str,
    target = str,
    is_directory = bool,
)

ensure_file_symlink_analyze = data_only_feature_analysis_fn(symlink_record, feature_type = "ensure_file_symlink")
ensure_dir_symlink_analyze = data_only_feature_analysis_fn(symlink_record, feature_type = "ensure_file_symlink")
