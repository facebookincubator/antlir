# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

target_t = shape.shape(
    __I_AM_TARGET__ = True,
    name = shape.field(str),
    # This will always be left as an empty string until all consumers have been
    # rolled out with this as an optional field, then it can be removed.
    path = shape.field(shape.path, optional = True),
    __thrift = {
        1: "name",
        2: "path",
    },
)
