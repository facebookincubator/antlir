# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

user_t = shape.shape(
    name = str,
    id = shape.field(int, optional = True),
    primary_group = str,
    supplementary_groups = shape.list(str),
    shell = shape.path,
    home_dir = shape.path,
    comment = shape.field(str, optional = True),
)

group_t = shape.shape(
    name = str,
    id = shape.field(int, optional = True),
)

usermod_t = shape.shape(
    username = str,
    add_supplementary_groups = shape.field(shape.list(str), default = []),
)
