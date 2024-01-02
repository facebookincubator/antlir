# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

def genrule(
        *,
        cmd: list[str | Select] | Select,
        user: str | Select = "nobody",
        boot: bool | Select = False,
        bind_repo_ro: bool | Select = False,
        mount_platform: bool | Select = False) -> ParseTimeFeature:
    return ParseTimeFeature(
        feature_type = "genrule",
        plugin = antlir2_dep("//antlir/antlir2/features/genrule:genrule"),
        kwargs = {
            "bind_repo_ro": bind_repo_ro,
            "boot": boot,
            "mount_platform": mount_platform,
            "user": user,
        },
        args = {
            "cmd_" + str(idx): cmd
            for idx, cmd in enumerate(cmd)
        },
    )

genrule_record = record(
    cmd = list[ResolvedStringWithMacros],
    user = str,
    boot = bool,
    bind_repo_ro = bool,
    mount_platform = bool,
)

def _genrule_impl(ctx: AnalysisContext) -> list[Provider]:
    cmd = {int(key.removeprefix("cmd_")): val for key, val in ctx.attrs.args.items() if key.startswith("cmd_")}
    cmd = [val for _key, val in sorted(cmd.items())]
    return [DefaultInfo(), FeatureAnalysis(
        feature_type = "genrule",
        data = genrule_record(
            cmd = cmd,
            user = ctx.attrs.user,
            boot = ctx.attrs.boot,
            # The repo is considered part of the platform
            bind_repo_ro = ctx.attrs.bind_repo_ro or ctx.attrs.mount_platform,
            mount_platform = ctx.attrs.mount_platform,
        ),
        build_phase = BuildPhase("genrule"),
        plugin = ctx.attrs.plugin[FeaturePluginInfo],
    )]

genrule_rule = rule(
    impl = _genrule_impl,
    attrs = {
        # TODO: just use attrs.arg() by itself
        "args": attrs.dict(attrs.string(), attrs.arg()),
        "bind_repo_ro": attrs.bool(),
        "boot": attrs.bool(),
        "mount_platform": attrs.bool(),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "user": attrs.string(),
    },
)
