# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

target_t = shape.shape(
    __I_AM_TARGET__ = True,
    name = shape.field(str, optional = True),
    path = shape.path,
)
