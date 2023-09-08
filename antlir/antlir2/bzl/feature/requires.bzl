# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn")

def requires(
        *,
        files: list[str] = [],
        groups: list[str] = [],
        users: list[str] = []) -> ParseTimeFeature:
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
        impl = antlir2_dep("features:requires"),
        kwargs = {
            "files": files,
            "groups": groups,
            "users": users,
        },
    )

requires_record = record(
    files = list[str],
    users = list[str],
    groups = list[str],
)

requires_analyze = data_only_feature_analysis_fn(
    requires_record,
    feature_type = "requires",
)
