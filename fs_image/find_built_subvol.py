#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import sys

from .artifacts_dir import ensure_per_repo_artifacts_dir_exists
from .compiler.subvolume_on_disk import SubvolumeOnDisk
from .fs_utils import Path
from .subvol_utils import Subvol
from .volume_for_repo import get_volume_for_current_repo


# NB: Memoizing this function would be pretty reasonable.
def volume_dir(path_in_repo=None):
    if path_in_repo is None:
        # This is the right default for unit tests and other things that get
        # run directly from the repo's `buck-out`.
        path_in_repo = sys.argv[0]
    lots_of_bytes = 1e8  # Our loopback is sparse, so just make it huge.
    return get_volume_for_current_repo(
        lots_of_bytes, ensure_per_repo_artifacts_dir_exists(path_in_repo),
    )


def subvolumes_dir(path_in_repo=None):
    return os.path.join(volume_dir(path_in_repo), 'targets')

_get_subvolumes_dir = subvolumes_dir


def find_built_subvol(
    layer_output, *, path_in_repo=None, subvolumes_dir=None,
):
    # It's OK for both to be None (uses the current file to find repo), but
    # it's not OK to set both.
    assert (path_in_repo is None) or (subvolumes_dir is None)
    with open(Path(layer_output) / 'layer.json') as infile:
        return Subvol(
            SubvolumeOnDisk.from_json_file(
                infile,
                subvolumes_dir if subvolumes_dir
                    else _get_subvolumes_dir(path_in_repo),
            ).subvolume_path(),
            already_exists=True,
        )


# The manual test was as follows:
#
#   $ (buck run fs_image:find-built-subvol -- "$(
#       buck targets --show-output fs_image/compiler/tests:hello_world_base |
#         cut -f 2- -d\
#     )") 2> /dev/null
#   /.../buck-image-out/volume/targets/hello_world_base:JBc1y_8.PoDr.dwGz/volume
if __name__ == '__main__':   # pragma: no cover
    # The newline is for bash's $() to strip.  This way even paths ending in
    # \n should work correctly.
    sys.stdout.buffer.write(find_built_subvol(sys.argv[1]).path() + b'\n')
