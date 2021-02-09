#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys

from .artifacts_dir import find_artifacts_dir
from .compiler.subvolume_on_disk import SubvolumeOnDisk
from .fs_utils import Path
from .subvol_utils import Subvol


# NB: Memoizing this function would be pretty reasonable.
def volume_dir(path_in_repo=None) -> Path:
    return find_artifacts_dir(path_in_repo) / "volume"


def _get_subvolumes_dir(path_in_repo=None) -> Path:
    return volume_dir(path_in_repo) / "targets"


def find_built_subvol(
    layer_output, *, path_in_repo=None, subvolumes_dir=None
) -> Subvol:
    # It's OK for both to be None (uses the current file to find repo), but
    # it's not OK to set both.
    assert (path_in_repo is None) or (subvolumes_dir is None)
    with open(Path(layer_output) / "layer.json") as infile:
        return Subvol(
            SubvolumeOnDisk.from_json_file(
                infile, str(subvolumes_dir or _get_subvolumes_dir(path_in_repo))
            ).subvolume_path(),
            already_exists=True,
        )


# The manual test was as follows:
#
#   $ (buck run antlir:find-built-subvol -- "$(
#       buck targets --show-output antlir/compiler/tests:hello_world_base |
#         cut -f 2- -d\
#     )") 2> /dev/null
#   /.../buck-image-out/volume/targets/hello_world_base:JBc1y_8.PoDr.dwGz/volume
if __name__ == "__main__":  # pragma: no cover
    # The newline is for bash's $() to strip.  This way even paths ending in
    # \n should work correctly.
    sys.stdout.buffer.write(find_built_subvol(sys.argv[1]).path() + b"\n")
