#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import pwd
import stat
import unittest


class Test(unittest.TestCase):
    def test_user(self) -> None:
        if os.environ["TEST_USER"] == "root":
            self.assertEqual(0, os.getuid())
        else:
            ent = pwd.getpwuid(os.getuid())
            self.assertEqual(ent.pw_name, os.environ["TEST_USER"])

    def test_env_propagated(self) -> None:
        self.assertEqual("1", os.getenv("ANTLIR2_TEST"))

    def test_json_env_quoting(self) -> None:
        self.assertEqual({"foo": "bar"}, json.loads(os.getenv("JSON_ENV")))

    def dev_null(self) -> None:
        st = os.stat("/dev/null")
        self.assertTrue(st.st_mode & stat.S_IFCHR, "/dev/null should be char device")
        self.assertEqual(
            st.st_rdev & stat.S_IFCHR,
            os.makedev(1, 3),
            "/dev/null device number is wrong",
        )
