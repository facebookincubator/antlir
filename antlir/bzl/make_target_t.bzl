# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:target.shape.bzl", "target_t")
load("//antlir/bzl:target_helpers.bzl", "normalize_target")

def make_target_t(label: str) -> target_t:
    return target_t(
        name = normalize_target(label),
        path = "",
    )
