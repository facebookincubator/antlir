# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

def _symlink_feature(*, link, target, feature_type) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = feature_type,
        kwargs = {
            "link": link,
            "target": target,
        },
    )

ensure_file_symlink = partial(_symlink_feature, feature_type = "ensure_file_symlink")
ensure_dir_symlink = partial(_symlink_feature, feature_type = "ensure_dir_symlink")

def symlink_to_json(
        link: str.type,
        target: str.type,
        sources: {str.type: "artifact"},
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    return {
        "dest": link,
        "source": target,
    }
