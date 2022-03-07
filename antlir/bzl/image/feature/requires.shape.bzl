# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

requires_t = shape.shape(
    users = shape.field(shape.list(str), optional = True),
    groups = shape.field(shape.list(str), optional = True),
)
