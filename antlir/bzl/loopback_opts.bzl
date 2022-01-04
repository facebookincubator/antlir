# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load(":constants.bzl", "REPO_CFG")
load(":loopback_opts.shape.bzl", "loopback_opts_t")
load(":shape.bzl", "shape")
load(":structs.bzl", "structs")

def _new_loopback_opts_t(
        minimize_size = None,
        **kwargs):
    # Turn on minimize if we haven't been explicitly told one way or
    # the other *and* the artifacts we are building don't require the repository
    if not REPO_CFG.artifacts_require_repo and minimize_size == None:
        minimize_size = True

    return shape.new(
        loopback_opts_t,
        minimize_size = minimize_size or False,
        **kwargs
    )

def normalize_loopback_opts(loopback_opts):
    if not loopback_opts:
        loopback_opts = {}
    if types.is_dict(loopback_opts):
        return _new_loopback_opts_t(**loopback_opts)
    return _new_loopback_opts_t(**structs.to_dict(loopback_opts))
