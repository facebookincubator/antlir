# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

mount_t = shape.shape(
    mount_config = shape.field(shape.dict(
        str,
        shape.union(
            bool,
            str,
            shape.dict(
                str,
                str,
            ),
        ),
    ), optional = True),
    mountpoint = shape.field(str, optional = True),
    target = shape.field(shape.dict(str, str), optional = True),
)
