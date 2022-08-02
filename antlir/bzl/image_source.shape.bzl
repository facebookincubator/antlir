# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")

image_source_t = shape.shape(
    source = shape.field(target_t, optional = True),
    layer = shape.field(target_t, optional = True),
    path = shape.field(shape.path, optional = True),
    content_hash = shape.field(str, optional = True),
)
