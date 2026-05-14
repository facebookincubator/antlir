# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")

def build_environment(*, path: str):
    return ParseTimeFeature(
        feature_type = "test_only_features/build_environment",
        plugin = "antlir//antlir/antlir2/features/test_only_features/build_environment:build_environment",
        kwargs = {
            "path": path,
        },
    )

build_environment_rule = data_only_feature_rule(
    feature_attrs = {
        "path": attrs.string(),
    },
    feature_type = "test_only_features/build_environment",
)
