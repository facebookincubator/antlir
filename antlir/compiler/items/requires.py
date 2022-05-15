#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from typing import Generator

from antlir.bzl.image.feature.requires import requires_t
from antlir.compiler.requires_provides import (
    RequireFile,
    RequireGroup,
    Requirement,
    RequireUser,
)

from .common import ImageItem


class RequiresItem(requires_t, ImageItem):
    def provides(self):
        return []

    def requires(self) -> Generator[Requirement, None, None]:
        for user in self.users or []:
            yield RequireUser(user)
        for user in self.groups or []:
            yield RequireGroup(user)
        for f in self.files or []:
            yield RequireFile(f)
