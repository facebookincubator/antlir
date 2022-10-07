# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def ensure_subdirs_exist(
        *,
        subdirs_to_create: str.type,
        mode: [int.type, str.type] = 0o755,
        user: str.type = "root",
        group: str.type = "root") -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "ensure_dirs_exist",
        kwargs = {
            "must_exist": must_exist,
            "path": path,
        },
    )

def ensure_dirs_exist(
        *,
        dirs: str.type,
        subdirs_to_create: str.type,
        mode: [int.type, str.type] = 0o755,
        user: str.type = "root",
        group: str.type = "root") -> InlineFeatureInfo.type:
    return ensure_subdirs_exist(
        into_dir = "/",
        subdirs_to_create = dirs,
        mode = mode,
        user = user,
        group = group,
    )

def ensure_dirs_exist_to_json(
        *,
        into_dir: str.type,
        mode: int.type,
        user: str.type,
        group: str.type,
        sources: {str.type: "artifact"},
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    return {
        "group": group,
        "into_dir": into_dir,
        "mode": mode,
        "subdirs_to_create": subdirs_to_create,
        "user": user,
    }
