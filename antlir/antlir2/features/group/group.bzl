# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")

def group_add(
        *,
        gid: int | Select,
        groupname: str | Select):
    """
    Add a group entry to /etc/group

    Group add semantics generally follow `groupadd`. If groupname or GID
    conflicts with existing entries, image build will fail.
    """
    return ParseTimeFeature(
        feature_type = "group",
        plugin = "antlir//antlir/antlir2/features/group:group",
        kwargs = {
            "gid": gid,
            "groupname": groupname,
        },
    )

group_rule = data_only_feature_rule(
    feature_attrs = {
        "gid": attrs.int(),
        "groupname": attrs.string(),
    },
    feature_type = "group",
)
