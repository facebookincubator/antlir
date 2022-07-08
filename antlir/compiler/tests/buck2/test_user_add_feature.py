#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import unittest

from .helpers import generate_group_str, generate_user_str


class UserFeatureTest(unittest.TestCase):
    def test_user_add(self):
        users = (
            subprocess.check_output(["cat", "/etc/passwd"], text=True)
            .strip()
            .split("\n")
        )

        self.assertIn(
            generate_user_str(
                user_name="test_user_1",
                password="x",
                uid="1345",
                gid="1234",
                comment="foo",
                home_dir="/home/test_user_1",
                shell="/bin/bash",
            ),
            users,
        )
        self.assertIn(
            generate_user_str(
                user_name="test_user_2",
                password="x",
                uid="2456",
                gid="2345",
                home_dir="/home/test_user_2",
                shell="/sbin/nologin",
            ),
            users,
        )

        groups = (
            subprocess.check_output(["cat", "/etc/group"], text=True)
            .strip()
            .split("\n")
        )

        self.assertIn(
            generate_group_str(
                group_name="test_group_1",
                password="x",
                gid="1234",
                user_list=["test_user_2"],
            ),
            groups,
        )
        self.assertIn(
            generate_group_str(
                group_name="test_group_2", password="x", gid="2345"
            ),
            groups,
        )
