# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

loopback_opts_t = shape.shape(
    # Size of the target image in MiB
    size_mb = shape.field(int, optional = True),
    label = shape.field(str, optional = True),
    # vfat-only options
    fat_size = shape.field(int, optional = True),
)
