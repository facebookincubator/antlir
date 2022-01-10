# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target_tagger.shape.bzl", "target_tagged_image_source_t")

tarball_t = shape.shape(
    force_root_ownership = shape.field(bool, optional = True),
    into_dir = shape.path,
    source = target_tagged_image_source_t,
)
