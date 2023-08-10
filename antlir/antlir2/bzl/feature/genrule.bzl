# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn")

def genrule(
        *,
        cmd: list[str | Select] | Select,
        user: str | Select = "nobody",
        boot: bool | Select = False,
        bind_repo_ro: bool | Select = False) -> ParseTimeFeature.type:
    return ParseTimeFeature(
        feature_type = "genrule",
        impl = "//antlir/antlir2/features:genrule",
        kwargs = {
            "bind_repo_ro": bind_repo_ro,
            "boot": boot,
            "cmd": cmd,
            "user": user,
        },
    )

genrule_record = record(
    cmd = list[str],
    user = str,
    boot = bool,
    bind_repo_ro = bool,
)

genrule_analyze = data_only_feature_analysis_fn(
    genrule_record,
    feature_type = "genrule",
    build_phase = BuildPhase("genrule"),
)
