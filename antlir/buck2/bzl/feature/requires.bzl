# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

def requires(
        *,
        files: [str.type] = [],
        groups: [str.type] = [],
        users: [str.type] = []) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "requires",
        kwargs = {
            "files": files,
            "groups": groups,
            "users": users,
        },
    )

def requires_to_json(
        files: [str.type],
        users: [str.type],
        groups: [str.type],
        sources: {str.type: "artifact"},
        deps: {str.type: "dependency"}) -> {str.type: ""}:
    return {
        "files": files,
        "groups": groups,
        "users": users,
    }
