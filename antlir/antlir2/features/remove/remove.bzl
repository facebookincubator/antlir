# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/features:feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")

def remove(
        *,
        path: str,
        must_exist: bool = True,
        must_be_empty: bool = False):
    """
    Recursively remove a file or directory

    These are allowed to remove paths inherited from the parent layer, or those
    installed in this layer.

    By default, it is an error if the specified path is missing from the image,
    though this can be avoided by setting `must_exist=False`.
    """
    return ParseTimeFeature(
        feature_type = "remove",
        plugin = "antlir//antlir/antlir2/features/remove:remove",
        kwargs = {
            "must_be_empty": must_be_empty,
            "must_exist": must_exist,
            "path": path,
        },
    )

remove_rule = data_only_feature_rule(
    feature_type = "remove",
    feature_attrs = {
        "build_phase": attrs.enum(BuildPhase.values(), default = "remove"),
        "must_be_empty": attrs.bool(),
        "must_exist": attrs.bool(),
        "path": attrs.string(),
    },
    build_phase = BuildPhase("remove"),
)
