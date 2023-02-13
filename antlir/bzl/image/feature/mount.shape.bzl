# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

build_source_t = shape.shape(
    type = str,
    source = shape.path,
)

mount_config_t = shape.shape(
    build_source = build_source_t,
    default_mountpoint = shape.path,
    is_directory = bool,
)

mount_spec_t = shape.shape(
    mount_config = shape.field(mount_config_t, optional = True),
    mountpoint = shape.field(shape.path, optional = True),
    target = shape.field(shape.dict(str, str), optional = True),
)
