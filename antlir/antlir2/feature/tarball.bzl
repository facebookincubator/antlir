# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":feature_info.bzl", "InlineFeatureInfo")

def tarball(
        *,
        src: str.type,
        into_dir: str.type,
        force_root_ownership: bool.type = False) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "tarball",
        sources = {
            "source": src,
        },
        kwargs = {
            "force_root_ownership": force_root_ownership,
            "into_dir": into_dir,
        },
    )

def tarball_to_json(
        into_dir: str.type,
        force_root_ownership: bool.type,
        sources: {str.type: "artifact"}) -> {str.type: ""}:
    return {
        "force_root_ownership": force_root_ownership,
        "into_dir": into_dir,
        "source": sources["source"],
    }
