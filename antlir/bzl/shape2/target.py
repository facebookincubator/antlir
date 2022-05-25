# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import Optional

import pydantic
from antlir.cli import normalize_buck_path
from antlir.fs_utils import Path
from antlir.shape import Shape


# pyre-fixme[13]: Attribute `name` is never initialized.
# pyre-fixme[13]: Attribute `path` is never initialized.
class target_t(Shape):
    name: Optional[str] = None
    path: Path

    @pydantic.validator("path")
    # pyre-fixme[2]: Parameter must be annotated.
    def normalize_path(cls, v) -> Path:  # noqa: B902 - This _is_ a class method
        return normalize_buck_path(v)
