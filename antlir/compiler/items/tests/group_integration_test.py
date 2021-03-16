#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess

from antlir.find_built_subvol import find_built_subvol
from antlir.fs_utils import Path
from antlir.nspawn_in_subvol.args import PopenArgs, new_nspawn_opts
from antlir.nspawn_in_subvol.nspawn import run_nspawn
from antlir.subvol_utils import Subvol, TempSubvolumes

from ..group import GroupItem
from .common import BaseItemTestCase


def _getent_group(layer: Subvol, name: str):
    cp, _ = run_nspawn(
        new_nspawn_opts(
            cmd=["getent", "group", name],
            layer=layer,
        ),
        PopenArgs(
            stdout=subprocess.PIPE,
        ),
    )
    assert cp.returncode == 0
    return cp.stdout


class GroupItemIntegrationTestCase(BaseItemTestCase):
    def test_group_item_in_subvol(self):
        layer = find_built_subvol(Path(__file__).dirname() / "base-layer")
        items = [
            GroupItem(from_target="t", name="foo"),
            GroupItem(from_target="t", name="foo2"),
            GroupItem(from_target="t", name="bar", id=1234),
            GroupItem(from_target="t", name="baz"),
        ]

        with TempSubvolumes() as ts:
            sv = ts.snapshot(layer, "add_groups")
            for item in items:
                item.build(sv)

            self.assertEqual(b"foo:x:1000:\n", _getent_group(sv, "foo"))
            self.assertEqual(b"foo2:x:1001:\n", _getent_group(sv, "foo2"))
            self.assertEqual(b"bar:x:1234:\n", _getent_group(sv, "bar"))
            self.assertEqual(b"baz:x:1235:\n", _getent_group(sv, "baz"))

    def test_check_groups_added_layer(self):
        layer = find_built_subvol(Path(__file__).dirname() / "groups-added")
        self.assertRegex(_getent_group(layer, "foo"), rb"^foo:x:\d+:\n$")
        self.assertEqual(_getent_group(layer, "leet"), b"leet:x:1337:\n")
