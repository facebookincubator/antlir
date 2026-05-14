# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature", "new_feature_rule")

def oci_label(*, key: str, value: str):
    return ParseTimeFeature(
        feature_type = "oci/oci_label",
        plugin = "antlir//antlir/antlir2/features/oci/oci_label:oci_label",
        kwargs = {
            "key": key,
            "value": value,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    fact_json = ctx.actions.write_json(
        "facts.json",
        [
            struct(
                type = "antlir2_packager::oci::OciLabel",
                # instead of just using the key, use the full key=value pair to be able
                # to see any conflicts later on
                key = "=".join([ctx.attrs.key, ctx.attrs.value]),
                value = struct(
                    key = ctx.attrs.key,
                    value = ctx.attrs.value,
                ),
            ),
        ],
        has_content_based_path = False,
    )

    return [
        DefaultInfo(),
        FeatureAnalysis(
            data = struct(
                key = ctx.attrs.key,
                value = ctx.attrs.value,
            ),
            feature_type = "oci/oci_label",
            plugin = ctx.attrs.plugin,
            extend_facts_json = [fact_json],
        ),
    ]

oci_label_rule = new_feature_rule(
    impl = _impl,
    attrs = {
        "key": attrs.string(),
        "value": attrs.string(),
    },
)
