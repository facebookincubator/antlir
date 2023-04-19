# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "ParseTimeFeature", "data_only_feature_analysis_fn")

types.lint_noop()

def genrule(
        *,
        cmd: types.or_selector([str.type]),
        user: types.or_selector(str.type),
        bind_repo_ro: types.or_selector(bool.type) = False,
        boot: types.or_selector(bool.type) = False) -> ParseTimeFeature.type:
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
    bind_repo_ro = bool.type,
    boot = bool.type,
)

genrule_analyze = data_only_feature_analysis_fn(genrule_record)
