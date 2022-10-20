# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import Optional

from antlir.fs_utils import Path

def find_built_subvol_internal(
    layer_output: Path,
    subvolumes_dir: Optional[Path],
    buck_root: Path,
) -> Path: ...

class SubvolNotFound(Exception): ...
