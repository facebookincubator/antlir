# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")

def usermod(
        *,
        username: str | Select,
        add_supplementary_groups: list[str | Select] | Select = []):
    """
    Modify an existing entry in the /etc/passwd and /etc/group databases
    """
    return ParseTimeFeature(
        feature_type = "user_mod",
        plugin = "antlir//antlir/antlir2/features/usermod:usermod",
        kwargs = {
            "add_supplementary_groups": add_supplementary_groups,
            "username": username,
        },
    )

usermod_rule = data_only_feature_rule(
    feature_attrs = {
        "add_supplementary_groups": attrs.list(attrs.string()),
        "username": attrs.string(),
    },
    feature_type = "user_mod",
)
