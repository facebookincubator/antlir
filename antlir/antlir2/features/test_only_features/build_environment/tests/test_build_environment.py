# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

# pyre-strict

import json
import unittest


class TestBuildEnvironment(unittest.TestCase):
    def test_file_exists_and_is_valid_json(self) -> None:
        with open("/.build_environment.json") as f:
            data = json.load(f)

        self.assertIsInstance(data, dict)
        self.assertIn("hostname", data)
        self.assertIn("env", data)

    def test_hostname_is_nonempty(self) -> None:
        with open("/.build_environment.json") as f:
            data = json.load(f)

        self.assertIsInstance(data["hostname"], str)
        self.assertNotEqual(data["hostname"], "")
        self.assertNotEqual(data["hostname"], "unknown")

    def test_env_is_dict(self) -> None:
        with open("/.build_environment.json") as f:
            data = json.load(f)

        self.assertIsInstance(data["env"], dict)

    def test_has_inside_re_worker(self) -> None:
        with open("/.build_environment.json") as f:
            data = json.load(f)

        self.assertIsInstance(data["inside_re_worker"], bool)
