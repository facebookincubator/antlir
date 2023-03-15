# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

def remove(
        *,
        path: str.type,
        must_exist: bool.type = True) -> InlineFeatureInfo.type:
    """
    Recursivel remove a file or directory

    These are allowed to remove paths inherited from the parent layer, or those
    installed in this layer.

    By default, it is an error if the specified path is missing from the image,
    though this can be avoided by setting `must_exist=False`.
    """
    return InlineFeatureInfo(
        feature_type = "remove",
        kwargs = {
            "must_exist": must_exist,
            "path": path,
        },
    )

remove_record = record(
    path = str.type,
    must_exist = bool.type,
)

remove_to_json = remove_record
