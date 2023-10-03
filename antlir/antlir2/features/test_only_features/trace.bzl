# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/bzl/feature:feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn")

def trace(
        *,
        msg: str) -> ParseTimeFeature:
    return ParseTimeFeature(
        feature_type = "test_only_features/trace",
        plugin = antlir2_dep("features/test_only_features:trace"),
        kwargs = {
            "msg": msg,
        },
    )

trace_record = record(
    msg = str,
)

trace_analyze = data_only_feature_analysis_fn(
    trace_record,
    feature_type = "test_only_features/trace",
)
