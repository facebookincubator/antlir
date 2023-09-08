# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/antlir2/bzl:macro_dep.bzl", "antlir2_dep")
load(":feature_info.bzl", "FeatureAnalysis", "ParseTimeFeature")

def genrule(
        *,
        cmd: list[str | Select] | Select,
        user: str | Select = "nobody",
        boot: bool | Select = False,
        bind_repo_ro: bool | Select = False,
        mount_platform: bool | Select = False) -> ParseTimeFeature:
    return ParseTimeFeature(
        feature_type = "genrule",
        impl = antlir2_dep("features:genrule"),
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

def genrule_analyze(
        user: str,
        boot: bool,
        bind_repo_ro: bool,
        mount_platform: bool,
        args: dict[str, str | ResolvedStringWithMacros]) -> FeatureAnalysis:
    cmd = {int(key.removeprefix("cmd_")): val for key, val in args.items() if key.startswith("cmd_")}
    cmd = [val for _key, val in sorted(cmd.items())]
    return FeatureAnalysis(
        feature_type = "genrule",
        data = genrule_record(
            cmd = cmd,
            user = user,
            boot = boot,
            # The repo is considered part of the platform
            bind_repo_ro = bind_repo_ro or mount_platform,
            mount_platform = mount_platform,
        ),
        build_phase = BuildPhase("genrule"),
    )
