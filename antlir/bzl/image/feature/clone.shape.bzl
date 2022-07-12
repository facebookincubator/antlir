# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_tagger.shape.bzl", "target_tagged_image_source_t")

clone_t = shape.shape(
    dest = shape.path,
    omit_outer_dir = bool,
    pre_existing_dest = bool,
    source = target_tagged_image_source_t,
    source_layer = shape.dict(str, str),
)
