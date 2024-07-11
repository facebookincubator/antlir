# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/features:defs.bzl", "FeaturePluginInfo")
load("//antlir/antlir2/features:feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

def genrule(
        *,
        cmd: list[str | Select] | Select | None = None,
        bash: str | Select | None = None,
        user: str | Select = "nobody",
        bind_repo_ro: bool | Select = False,
        mount_platform: bool | Select = False):
    if int(bool(cmd)) + int(bool(bash)) != 1:
        fail("Must provide exactly one of `cmd` or `bash`")
    return ParseTimeFeature(
        feature_type = "genrule",
        plugin = "antlir//antlir/antlir2/features/genrule:genrule",
        kwargs = {
            "bind_repo_ro": bind_repo_ro,
            "mount_platform": mount_platform,
            "user": user,
        },
        args = {
            "cmd_" + str(idx): cmd
            for idx, cmd in enumerate(cmd or [])
        } | ({
            "bash": bash,
        } if bash else {}),
    )

genrule_record = record(
    cmd = list[ResolvedStringWithMacros | list[str]],
    user = str,
    bind_repo_ro = bool,
    mount_platform = bool,
)

def _genrule_impl(ctx: AnalysisContext) -> list[Provider]:
    bash = ctx.attrs.args.pop("bash", None)
    if bash:
        cmd = [["/bin/bash", "-c"], bash]
    else:
        cmd = {int(key.removeprefix("cmd_")): val for key, val in ctx.attrs.args.items() if key.startswith("cmd_")}
        cmd = [val for _key, val in sorted(cmd.items())]
    return [DefaultInfo(), FeatureAnalysis(
        feature_type = "genrule",
        data = genrule_record(
            cmd = cmd,
            user = ctx.attrs.user,
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
        "mount_platform": attrs.bool(),
        "plugin": attrs.exec_dep(providers = [FeaturePluginInfo]),
        "user": attrs.string(),
    },
)
