# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:shape.bzl", "shape")

loopback_opts_t = shape.shape(
    # Size of the target image in MiB
    size_mb = shape.field(int, optional = True),
    label = shape.field(str, optional = True),
    # Note: These options are for btrfs loopbacks only. Ideally they would
    # be defined in their own shape type, but nested shape types
    # are hard to use from python because the type name is not
    # known.  Until that issue is fixed, we will just embed these
    # here.
    #
    # Set the default compression level to 1 to save CPU by default.
    compression_level = shape.field(int, default = 1),
    default_subvolume = shape.field(bool, default = False),
    seed_device = shape.field(bool, default = False),
    subvol_name = shape.field(str, optional = True),
    writable_subvolume = shape.field(bool, default = False),
    # vfat-only options
    fat_size = shape.field(int, optional = True),
)
