# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature", "new_feature_rule")

def oci_env(
        *,
        key: str,
        value: str):
    """
    Set an environment variable in the OCI container configuration.

    This feature allows setting environment variables that will be automatically
    available when the container starts, without needing to pass them via
    podman run --env flags.

    Args:
        key: The environment variable name (e.g., "HHVM_DISABLE_PERSONALITY")
        value: The environment variable value (e.g., "1")
    """
    return ParseTimeFeature(
        feature_type = "oci/oci_env",
        plugin = "antlir//antlir/antlir2/features/oci/oci_env:oci_env",
        kwargs = {
            "key": key,
            "value": value,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider] | Promise:
    fact_json = ctx.actions.write_json("facts.json", [
        struct(
            type = "antlir2_packager::oci::OciEnv",
            # use the full KEY=VALUE pair as the fact key to detect conflicts
            key = "=".join([ctx.attrs.key, ctx.attrs.value]),
            value = struct(
                key = ctx.attrs.key,
                value = ctx.attrs.value,
            ),
        ),
    ], has_content_based_path = False)

    return [
        DefaultInfo(),
        FeatureAnalysis(
            data = struct(
                key = ctx.attrs.key,
                value = ctx.attrs.value,
            ),
            feature_type = "oci/oci_env",
            plugin = ctx.attrs.plugin,
            extend_facts_json = [fact_json],
        ),
    ]

oci_env_rule = new_feature_rule(
    impl = _impl,
    attrs = {
        "key": attrs.string(),
        "value": attrs.string(),
    },
)
