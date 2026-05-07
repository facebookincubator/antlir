# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature", "new_feature_rule")

def oci_stop_signal(*, signal: str):
    return ParseTimeFeature(
        feature_type = "oci/oci_stop_signal",
        plugin = "antlir//antlir/antlir2/features/oci/oci_stop_signal:oci_stop_signal",
        kwargs = {
            "stop_signal": signal,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    fact_json = ctx.actions.write_json("facts.json", [
        struct(
            type = "antlir2_packager::oci::OciStopSignal",
            key = ctx.attrs.stop_signal,
            value = struct(
                stop_signal = ctx.attrs.stop_signal,
            ),
        ),
    ], has_content_based_path = False)

    return [
        DefaultInfo(),
        FeatureAnalysis(
            data = struct(
                stop_signal = ctx.attrs.stop_signal,
            ),
            feature_type = "oci/oci_stop_signal",
            plugin = ctx.attrs.plugin,
            extend_facts_json = [fact_json],
        ),
    ]

oci_stop_signal_rule = new_feature_rule(
    impl = _impl,
    attrs = {
        "stop_signal": attrs.string(),
    },
)
