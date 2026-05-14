# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature", "new_feature_rule")

def oci_user(*, user: str):
    """
    Set the default user in the OCI container configuration.

    Args:
        user: The username or UID to run as by default (e.g., "root", "1000",
            "user:group", or "1000:1000")
    """
    return ParseTimeFeature(
        feature_type = "oci/oci_user",
        plugin = "antlir//antlir/antlir2/features/oci/oci_user:oci_user",
        kwargs = {
            "user": user,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    fact_json = ctx.actions.write_json(
        "facts.json",
        [
            struct(
                type = "antlir2_packager::oci::OciUser",
                key = ctx.attrs.user,
                value = struct(
                    user = ctx.attrs.user,
                ),
            ),
        ],
        has_content_based_path = False,
    )

    return [
        DefaultInfo(),
        FeatureAnalysis(
            data = struct(
                user = ctx.attrs.user,
            ),
            feature_type = "oci/oci_user",
            plugin = ctx.attrs.plugin,
            extend_facts_json = [fact_json],
        ),
    ]

oci_user_rule = new_feature_rule(
    impl = _impl,
    attrs = {
        "user": attrs.string(),
    },
)
