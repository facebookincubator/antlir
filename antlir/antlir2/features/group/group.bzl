# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load(
    "//antlir/antlir2/features:feature_info.bzl",
    "FeatureAnalysis",
    "ParseTimeFeature",
)
load("//antlir/buck2/bzl:ensure_single_output.bzl", "ensure_single_output")

def group_add(
        *,
        groupname: str | Select,
        gid: int | Select | None = None,
        uidmap: str = "default"):
    """
    Add a group entry to /etc/group

    Group add semantics generally follow `groupadd`. If groupname or GID
    conflicts with existing entries, image build will fail.
    """
    return ParseTimeFeature(
        feature_type = "group",
        plugin = "antlir//antlir/antlir2/features/group:group",
        deps = {
            "uidmap": ("antlir//antlir/uidmaps:" + uidmap) if ":" not in uidmap else uidmap,
        },
        kwargs = {
            "gid": gid,
            "groupname": groupname,
        },
    )

def _impl(ctx: AnalysisContext) -> list[Provider]:
    uidmap = ensure_single_output(ctx.attrs.uidmap)
    return [
        DefaultInfo(),
        FeatureAnalysis(
            feature_type = "group",
            data = struct(
                gid = ctx.attrs.gid,
                groupname = ctx.attrs.groupname,
                uidmap = uidmap,
            ),
            build_phase = BuildPhase("compile"),
            required_artifacts = [uidmap],
            plugin = ctx.attrs.plugin[FeaturePluginInfo],
        ),
    ]

group_rule = rule(
    impl = _impl,
    attrs = {
        "gid": attrs.option(attrs.int()),
        "groupname": attrs.string(),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "uidmap": attrs.dep(),
    },
)
