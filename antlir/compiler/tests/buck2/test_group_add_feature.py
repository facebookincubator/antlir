#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import unittest

from .generate_usergroup_str import generate_group_str


class GroupFeatureTest(unittest.TestCase):
    def test_group_add(self):
        groups = (
            subprocess.check_output(["cat", "/etc/group"], text=True)
            .strip()
            .split("\n")
        )

        self.assertIn(
            generate_group_str(
                group_name="test_group_1", password="x", gid="1234"
            ),
            groups,
        )
        self.assertIn(
            generate_group_str(
                group_name="test_group_2", password="x", gid="2345"
            ),
            groups,
        )
