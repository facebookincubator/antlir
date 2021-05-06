#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes
from antlir.tests.layer_resource import layer_resource_subvol

from ..group import GroupItem
from ..user import UserItem
from .common import BaseItemTestCase, getent


class UserItemIntegrationTestCase(BaseItemTestCase):
    def test_user_item_in_subvol(self):
        layer = layer_resource_subvol(__package__, "base-layer")
        items = [
            GroupItem(from_target="t", name="foo"),
            UserItem(
                from_target="t",
                name="foo",
                primary_group="foo",
                supplementary_groups=[],
                home_dir="/home/foo",
                shell="/bin/bash",
                comment="new user",
            ),
        ]

        with TempSubvolumes() as ts:
            sv = ts.snapshot(layer, "add_user")
            for item in items:
                item.build(sv)

            self.assertEqual(b"foo:x:1000:\n", getent(sv, "group", "foo"))
            self.assertEqual(
                b"foo:x:1000:1000:new user:/home/foo:/bin/bash\n",
                getent(sv, "passwd", "foo"),
            )

    def test_check_groups_added_layer(self):
        layer = layer_resource_subvol(__package__, "users-added")
        self.assertRegex(
            getent(layer, "group", "newuser"), rb"^newuser:x:\d+:\n$"
        )
        self.assertRegex(
            getent(layer, "passwd", "newuser"),
            rb"^newuser:x:\d+:\d+::/home/newuser:/bin/bash\n$",
        )
