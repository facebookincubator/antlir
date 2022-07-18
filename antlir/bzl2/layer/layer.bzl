# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load(":from_package.bzl", "layer_from_package")
load(":genrule.bzl", "layer_genrule")
load(":new.bzl", "layer_new")

layer = struct(
    from_package = layer_from_package,
    genrule = layer_genrule,
    new = layer_new,
)
