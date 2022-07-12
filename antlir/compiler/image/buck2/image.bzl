# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/compiler/image/buck2:image_genrule_layer.bzl", "image_genrule_layer")
load("//antlir/compiler/image/buck2:image_layer.bzl", "image_layer")
load("//antlir/compiler/image/buck2:image_layer_from_package.bzl", "image_layer_from_package")

image = struct(
    layer = image_layer,
    genrule_layer = image_genrule_layer,
    layer_from_package = image_layer_from_package,
)
