#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import sys
from typing import Optional

from antlir.find_built_subvol_rs import find_built_subvol_internal
from antlir.fs_utils import Path
from antlir.subvol_utils import Subvol


def find_built_subvol(
    layer_output, *, path_in_repo=None, subvolumes_dir: Optional[Path] = None
) -> Subvol:
    # It's OK for both to be None (uses the current file to find repo), but
    # it's not OK to set both.
    assert (path_in_repo is None) or (subvolumes_dir is None)

    layer_output = Path(layer_output).abspath()

    subvol = find_built_subvol_internal(
        layer_output,
        subvolumes_dir,
        path_in_repo,
    )

    return Subvol(path=subvol, already_exists=True)


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
