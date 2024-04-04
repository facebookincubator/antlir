# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

layer_mount_record = record(
    mountpoint = str,
    subvol_symlink = Artifact,
)

host_mount_record = record(
    mountpoint = str,
    src = str,
    is_directory = bool,
)

mount_record = record(
    layer = layer_mount_record | None,
    host = host_mount_record | None,
)
