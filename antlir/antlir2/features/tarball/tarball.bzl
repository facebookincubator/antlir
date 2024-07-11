# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

def tarball(
        *,
        src: str,
        into_dir: str,
        force_root_ownership: bool = False):
    return ParseTimeFeature(
        feature_type = "tarball",
        plugin = "antlir//antlir/antlir2/features/tarball:tarball",
        srcs = {
            "src": src,
        },
        kwargs = {
            "force_root_ownership": force_root_ownership,
            "into_dir": into_dir,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider]:
    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "tarball",
            data = struct(
                src = ctx.attrs.src,
                into_dir = ctx.attrs.into_dir,
                force_root_ownership = ctx.attrs.force_root_ownership,
            ),
            required_artifacts = [ctx.attrs.src],
            plugin = ctx.attrs.plugin[FeaturePluginInfo],
        ),
    ]

tarball_rule = rule(
    impl = _impl,
    attrs = {
        "force_root_ownership": attrs.bool(),
        "into_dir": attrs.string(),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "src": attrs.source(),
    },
)
