# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature", "new_feature_rule")

def oci_volume(*, path: str):
    return ParseTimeFeature(
        feature_type = "oci/oci_volume",
        plugin = "antlir//antlir/antlir2/features/oci/oci_volume:oci_volume",
        kwargs = {
            "path": path,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    fact_json = ctx.actions.write_json(
        "facts.json",
        [
            struct(
                type = "antlir2_packager::oci::OciVolume",
                key = ctx.attrs.path,
                value = struct(
                    path = ctx.attrs.path,
                ),
            ),
        ],
        has_content_based_path = False,
    )

    return [
        DefaultInfo(),
        FeatureAnalysis(
            data = struct(
                path = ctx.attrs.path,
            ),
            feature_type = "oci/oci_volume",
            plugin = ctx.attrs.plugin,
            extend_facts_json = [fact_json],
        ),
    ]

oci_volume_rule = new_feature_rule(
    impl = _impl,
    attrs = {
        "path": attrs.string(),
    },
)
