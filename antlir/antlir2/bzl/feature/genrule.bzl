# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/antlir2/bzl:build_phase.bzl", "BuildPhase")
load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn")

_STR_LIST_OR_SELECTOR = types.or_selector([str.type])
_STR_OR_SELECTOR = types.or_selector(str.type)
_BOOL_OR_SELECTOR = types.or_selector(bool.type)

types.lint_noop(_STR_LIST_OR_SELECTOR, _STR_OR_SELECTOR, _BOOL_OR_SELECTOR)

def genrule(
        *,
        cmd: _STR_LIST_OR_SELECTOR,
        user: _STR_OR_SELECTOR = "nobody",
        boot: _BOOL_OR_SELECTOR = False,
        bind_repo_ro: _BOOL_OR_SELECTOR = False) -> ParseTimeFeature.type:
    return ParseTimeFeature(
        feature_type = "genrule",
        kwargs = {
            "bind_repo_ro": bind_repo_ro,
            "boot": boot,
            "cmd": cmd,
            "user": user,
        },
    )

genrule_record = record(
    cmd = [str.type],
    user = str.type,
    boot = bool.type,
    bind_repo_ro = bool.type,
)

genrule_analyze = data_only_feature_analysis_fn(
    genrule_record,
    build_phase = BuildPhase("genrule"),
)
