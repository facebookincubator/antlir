# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")

script_t = shape.shape(
    prepare = shape.field(str, optional = True),
    build = str,
    install = str,
    patches = shape.list(str, optional = True),
)

dep_t = shape.shape(
    name = str,
    source = target_t,
    paths = shape.list(str),
)
