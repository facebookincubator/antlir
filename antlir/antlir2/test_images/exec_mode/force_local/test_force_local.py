# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import socket
import unittest


class TestForceLocal(unittest.TestCase):
    def setUp(self) -> None:
        with open("/.build_environment.json") as f:
            self.data = json.load(f)

    def test_build_environment_valid(self) -> None:
        self.assertIsInstance(self.data, dict)
        self.assertIn("hostname", self.data)
        self.assertIn("env", self.data)

    def test_built_on_same_host(self) -> None:
        build_hostname = self.data["hostname"]
        test_hostname = socket.gethostname()
        self.assertEqual(
            build_hostname,
            test_hostname,
            f"Build host ({build_hostname}) differs from test host ({test_hostname})"
            " - image was not built locally",
        )

    def test_not_built_on_remote_execution(self) -> None:
        env = self.data["env"]
        self.assertNotIn(
            "RE_PLATFORM",
            env,
            "RE_PLATFORM env var is set - image was built on remote execution,"
            " not locally",
        )
        self.assertFalse(self.data["inside_re_worker"])
