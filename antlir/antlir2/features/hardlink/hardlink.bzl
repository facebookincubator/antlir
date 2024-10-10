# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")

def hardlink(
        *,
        link: str | Select,
        target: str | Select):
    """
    Create a hardlink to a file.
    """
    return ParseTimeFeature(
        feature_type = "hardlink",
        plugin = "antlir//antlir/antlir2/features/hardlink:hardlink",
        kwargs = {
            "link": link,
            "target": target,
        },
    )

hardlink_rule = data_only_feature_rule(
    feature_attrs = {
        "link": attrs.string(),
        "target": attrs.string(),
    },
    feature_type = "hardlink",
)
