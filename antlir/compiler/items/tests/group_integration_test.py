#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from antlir.compiler.items.group import GroupItem
from antlir.compiler.items.tests.common import BaseItemTestCase, getent
from antlir.subvol_utils import TempSubvolumes, with_temp_subvols
from antlir.tests.layer_resource import layer_resource_subvol


class GroupItemIntegrationTestCase(BaseItemTestCase):
    @with_temp_subvols
    def test_group_item_in_subvol(self, ts: TempSubvolumes):
        layer = layer_resource_subvol(__package__, "base-layer")
        items = [
            GroupItem(from_target="t", name="foo"),
            GroupItem(from_target="t", name="foo2"),
            GroupItem(from_target="t", name="bar", id=1234),
            GroupItem(from_target="t", name="baz"),
        ]

        sv = ts.snapshot(layer, "add_groups")
        for item in items:
            item.build(sv)

        self.assertEqual(b"foo:x:1000:\n", getent(sv, "group", "foo"))
        self.assertEqual(b"foo2:x:1001:\n", getent(sv, "group", "foo2"))
        self.assertEqual(b"bar:x:1234:\n", getent(sv, "group", "bar"))
        self.assertEqual(b"baz:x:1235:\n", getent(sv, "group", "baz"))

    def test_check_groups_added_layer(self):
        layer = layer_resource_subvol(__package__, "groups-added")
        self.assertRegex(getent(layer, "group", "foo"), rb"^foo:x:\d+:\n$")
        self.assertEqual(getent(layer, "group", "leet"), b"leet:x:1337:\n")
