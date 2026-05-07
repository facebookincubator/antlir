# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature", "new_feature_rule")

def oci_exposed_port(*, port: str):
    return ParseTimeFeature(
        feature_type = "oci/oci_exposed_port",
        plugin = "antlir//antlir/antlir2/features/oci/oci_exposed_port:oci_exposed_port",
        kwargs = {
            "port": port,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    fact_json = ctx.actions.write_json("facts.json", [
        struct(
            type = "antlir2_packager::oci::OciExposedPort",
            key = ctx.attrs.port,
            value = struct(
                port = ctx.attrs.port,
            ),
        ),
    ], has_content_based_path = False)

    return [
        DefaultInfo(),
        FeatureAnalysis(
            data = struct(
                port = ctx.attrs.port,
            ),
            feature_type = "oci/oci_exposed_port",
            plugin = ctx.attrs.plugin,
            extend_facts_json = [fact_json],
        ),
    ]

oci_exposed_port_rule = new_feature_rule(
    impl = _impl,
    attrs = {
        "port": attrs.string(),
    },
)
