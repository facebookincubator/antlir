# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature", "new_feature_rule")

def oci_working_dir(*, working_dir: str):
    return ParseTimeFeature(
        feature_type = "oci/oci_working_dir",
        plugin = "antlir//antlir/antlir2/features/oci/oci_working_dir:oci_working_dir",
        kwargs = {
            "working_dir": working_dir,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    fact_json = ctx.actions.write_json("facts.json", [
        struct(
            type = "antlir2_packager::oci::OciWorkingDir",
            key = ctx.attrs.working_dir,
            value = struct(
                working_dir = ctx.attrs.working_dir,
            ),
        ),
    ], has_content_based_path = False)

    return [
        DefaultInfo(),
        FeatureAnalysis(
            data = struct(
                working_dir = ctx.attrs.working_dir,
            ),
            feature_type = "oci/oci_working_dir",
            plugin = ctx.attrs.plugin,
            extend_facts_json = [fact_json],
        ),
    ]

oci_working_dir_rule = new_feature_rule(
    impl = _impl,
    attrs = {
        "working_dir": attrs.string(),
    },
)
