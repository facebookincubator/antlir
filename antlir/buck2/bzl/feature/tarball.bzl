# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def tarball(*, src, dst, force_root_ownership = False) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "tarball",
        sources = {
            "source": src,
        },
        kwargs = {
            "dst": dst,
            "force_root_ownership": force_root_ownership,
        },
    )

def tarball_to_json(
        dst: str.type,
        force_root_ownership: bool.type,
        sources: {str.type: "artifact"},
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    return {
        "dest": dst,
        "force_root_ownership": force_root_ownership,
        "source": sources["source"],
    }
