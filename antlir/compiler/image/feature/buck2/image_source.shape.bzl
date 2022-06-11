# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

image_source_t = shape.shape(
    source = shape.field(shape.dict(str, str), optional = True),
    layer = shape.field(shape.dict(str, str), optional = True),
    path = shape.field(str, optional = True),
    generator = shape.field(shape.dict(str, str), optional = True),
    generator_args = shape.field(shape.list(str), optional = True),
    content_hash = shape.field(str, optional = True),
)
