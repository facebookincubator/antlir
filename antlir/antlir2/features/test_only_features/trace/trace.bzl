# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "ParseTimeFeature", "data_only_feature_rule")

def trace(
        *,
        msg: str):
    return ParseTimeFeature(
        feature_type = "test_only_features/trace",
        plugin = "antlir//antlir/antlir2/features/test_only_features/trace:trace",
        kwargs = {
            "msg": msg,
        },
    )

trace_rule = data_only_feature_rule(
    feature_attrs = {
        "msg": attrs.string(),
    },
    feature_type = "test_only_features/trace",
)
