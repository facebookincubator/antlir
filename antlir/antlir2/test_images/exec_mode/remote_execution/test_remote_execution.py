# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import unittest


class TestRemoteExecution(unittest.TestCase):
    def setUp(self) -> None:
        with open("/.build_environment.json") as f:
            self.data = json.load(f)

    def test_build_environment_valid(self) -> None:
        self.assertIsInstance(self.data, dict)
        self.assertIn("hostname", self.data)
        self.assertIn("env", self.data)

    def test_hostname_is_nonempty(self) -> None:
        self.assertIsInstance(self.data["hostname"], str)
        self.assertNotEqual(self.data["hostname"], "")
        self.assertNotEqual(self.data["hostname"], "unknown")

    def test_built_on_remote_execution(self) -> None:
        env = self.data["env"]
        self.assertIn(
            "RE_PLATFORM",
            env,
            "RE_PLATFORM env var missing - image was not built on remote execution",
        )
        self.assertNotEqual(env["RE_PLATFORM"], "")
        self.assertTrue(self.data["inside_re_worker"])
