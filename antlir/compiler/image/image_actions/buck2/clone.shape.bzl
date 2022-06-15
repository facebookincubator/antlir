# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load(
    "//antlir/compiler/image/feature/buck2:image_source.shape.bzl",
    "image_source_t",
)

clone_t = shape.shape(
    dest = shape.path,
    omit_outer_dir = bool,
    pre_existing_dest = bool,
    source = image_source_t,
    source_layer = shape.dict(str, str),
)
