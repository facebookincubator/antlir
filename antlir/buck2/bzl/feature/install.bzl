# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:stat.bzl", "stat")
load(":feature_info.bzl", "InlineFeatureInfo")

def install(
        *,
        src: str.type,
        dst: str.type,
        mode: [int.type, str.type, None] = None,
        user: str.type = "root",
        group: str.type = "root") -> InlineFeatureInfo.type:
    mode = stat.mode(mode) if mode else None
    return InlineFeatureInfo(
        feature_type = "install",
        sources = {
            "source": src,
        },
        kwargs = {
            "dest": dst,
            "group": group,
            "mode": stat.mode(mode) if mode else None,
            "user": user,
        },
    )

def install_to_json(
        dest: str.type,
        group: str.type,
        mode: [int.type, None],
        user: str.type,
        sources: {str.type: "artifact"},
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    return {
        "dest": dest,
        "group": group,
        "mode": mode,
        "source": sources["source"],
        "user": user,
    }
