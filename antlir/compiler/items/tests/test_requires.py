#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.compiler.items.requires import RequiresItem
from antlir.compiler.items.tests.common import BaseItemTestCase
from antlir.compiler.requires_provides import (
    RequireFile,
    RequireGroup,
    RequireUser,
)
from antlir.fs_utils import Path


class BuckRequiresTest(BaseItemTestCase):
    def test_user_groups_files(self) -> None:
        self._check_item(
            RequiresItem(
                from_target="t",
                users=["foo", "bar"],
                groups=["users"],
                files=[Path("/a/b")],
            ),
            set(),  # this item never provides anything
            {
                RequireGroup("users"),
                RequireUser("foo"),
                RequireUser("bar"),
                RequireFile(Path("/a/b")),
            },
        )
