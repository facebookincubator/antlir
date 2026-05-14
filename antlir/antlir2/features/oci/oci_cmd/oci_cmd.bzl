# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature", "new_feature_rule")

def oci_cmd(*, cmd: list[str]):
    return ParseTimeFeature(
        feature_type = "oci/oci_cmd",
        plugin = "antlir//antlir/antlir2/features/oci/oci_cmd:oci_cmd",
        kwargs = {
            "cmd": cmd,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    fact_json = ctx.actions.write_json(
        "facts.json",
        [
            struct(
                type = "antlir2_packager::oci::OciCmd",
                key = "\t".join(ctx.attrs.cmd),
                value = struct(
                    cmd = ctx.attrs.cmd,
                ),
            ),
        ],
        has_content_based_path = False,
    )

    return [
        DefaultInfo(),
        FeatureAnalysis(
            data = struct(
                cmd = ctx.attrs.cmd,
            ),
            feature_type = "oci/oci_cmd",
            plugin = ctx.attrs.plugin,
            extend_facts_json = [fact_json],
        ),
    ]

oci_cmd_rule = new_feature_rule(
    impl = _impl,
    attrs = {
        "cmd": attrs.list(attrs.string()),
    },
)
