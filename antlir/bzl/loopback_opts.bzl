# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("@bazel_skylib//lib:types.bzl", "types")
load(":loopback_opts.shape.bzl", "loopback_opts_t")
load(":structs.bzl", "structs")

def _new_loopback_opts_t(
        **kwargs):
    return loopback_opts_t(
        **kwargs
    )

def normalize_loopback_opts(loopback_opts):
    if not loopback_opts:
        loopback_opts = {}
    if types.is_dict(loopback_opts):
        return _new_loopback_opts_t(**loopback_opts)
    return _new_loopback_opts_t(**structs.to_dict(loopback_opts))
