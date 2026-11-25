# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# Eventually antlir2 hopes to support other storage formats, but for now only
# local subvolumes are supported
LayerContents = record(
    subvol_symlink = field(Artifact | None),  # symlink pointing to the built subvol
    # was the subvol built in rootless mode or not?
    subvol_symlink_rootless = field(bool),
    cad_stack = field(list[Artifact] | None, default = None),
    configured_working_format = str,
)
