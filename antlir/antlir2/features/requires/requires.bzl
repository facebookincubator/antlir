# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")

def requires(
        *,
        files: list[str] = [],
        groups: list[str] = [],
        users: list[str] = []):
    """
    Add rule-level requirements on image layers.

    Currently this supports requiring users, groups and files to exist in the layer
    being built. This feature doesn't materialize anything in the built image, but it
    will cause a compiler error if any of the required features that are
    requested do not exist in either the `parent_layer` or the layer being
    built.

    An example of a reasonable use-case of this functionality is defining a
    macro that generates systemd units that run as a specific user, where
    `requires` can be used for additional compile-time safety that the user,
    groups or files do indeed exist.
    """
    return ParseTimeFeature(
        feature_type = "requires",
        plugin = "antlir//antlir/antlir2/features/requires:requires",
        kwargs = {
            "files": files,
            "groups": groups,
            "users": users,
        },
    )

requires_rule = data_only_feature_rule(
    feature_attrs = {
        "files": attrs.list(attrs.string()),
        "groups": attrs.list(attrs.string()),
        "users": attrs.list(attrs.string()),
    },
    feature_type = "requires",
)
