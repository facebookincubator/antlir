# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

def remove(
        *,
        path: str.type,
        must_exist: bool.type = True) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "remove",
        kwargs = {
            "must_exist": must_exist,
            "path": path,
        },
    )

def remove_to_json(
        path: str.type,
        must_exist: bool.type) -> {str.type: ""}:
    return {
        "must_exist": must_exist,
        "path": path,
    }
