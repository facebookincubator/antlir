# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:container_opts.shape.bzl", "container_opts_t")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:types.bzl", "types")
load(":feature_info.bzl", "InlineFeatureInfo")

types.lint_noop(container_opts_t)

def genrule(
        *,
        cmd: [str.type],
        user: str.type,
        container_opts: types.shape(container_opts_t),
        bind_repo_ro: bool.type = False,
        boot: bool.type = False) -> InlineFeatureInfo.type:
    return InlineFeatureInfo(
        feature_type = "genrule",
        kwargs = {
            "bind_repo_ro": bind_repo_ro,
            "boot": boot,
            "cmd": cmd,
            "container_opts": shape.as_serializable_dict(container_opts),
            "user": user,
        },
    )

def genrule_to_json(
        cmd: [str.type],
        user: str.type,
        container_opts: {str.type: ""},
        bind_repo_ro: bool.type = False,
        boot: bool.type = False) -> {str.type: ""}:
    return {
        "bind_repo_ro": bind_repo_ro,
        "boot": boot,
        "cmd": cmd,
        "container_opts": container_opts,
        "user": user,
    }
