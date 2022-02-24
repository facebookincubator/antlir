# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.fs_utils import Path
from antlir.shape import Shape


# pyre-fixme[13]: Attribute `name` is never initialized.
# pyre-fixme[13]: Attribute `path` is never initialized.
class target_t(Shape):
    name: str
    path: Path
