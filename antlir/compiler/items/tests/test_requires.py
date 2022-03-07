#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.compiler.requires_provides import (
    RequireGroup,
    RequireUser,
)

from ..requires import RequiresItem
from .common import BaseItemTestCase


class UserItemTest(BaseItemTestCase):
    def test_usergroups(self):
        self._check_item(
            RequiresItem(
                from_target="t",
                users=["foo", "bar"],
                groups=["users"],
            ),
            set(),  # this item never provides anything
            {
                RequireGroup("users"),
                RequireUser("foo"),
                RequireUser("bar"),
            },
        )
