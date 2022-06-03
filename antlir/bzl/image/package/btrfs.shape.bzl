# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

load("//antlir/bzl:loopback_opts.shape.bzl", "loopback_opts_t")
load("//antlir/bzl:shape.bzl", "shape")
load("//antlir/bzl:target.shape.bzl", "target_t")

btrfs_subvol_t = shape.shape(
    layer = shape.field(target_t),
    writable = shape.field(bool, default = False),
)

btrfs_opts_t = shape.shape(
    compression_level = shape.field(int, default = 1),
    # Note that the key should be shape.path, but currently
    # that is unsupported
    subvols = shape.dict(str, btrfs_subvol_t),
    default_subvol = shape.field(shape.path, optional = True),
    seed_device = shape.field(bool, default = False),
    loopback_opts = shape.field(loopback_opts_t, optional = True),
    # free_mb is not percise, the actual amount of free space will vary.
    free_mb = shape.field(int, default = 0),
)
