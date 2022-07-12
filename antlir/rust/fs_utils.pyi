# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import Union

import antlir.fs_utils

def Path(input: Union[bytes, str]) -> antlir.fs_utils.Path: ...
def copy_file(src: antlir.fs_utils.Path, dst: antlir.fs_utils.Path) -> None: ...
