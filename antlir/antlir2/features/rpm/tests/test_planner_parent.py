# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import os
import unittest
from pathlib import Path


class PlannerParentTest(unittest.TestCase):
    def test_reuses_installed_provider(self) -> None:
        tx = json.loads(Path(os.environ["INSTALL"]).read_text())
        self.assertEqual(
            ["requires-any-foo"],
            [rpm["package"]["name"] for rpm in tx["install"]],
        )
        self.assertEqual([], tx["remove"])

    def test_removes_installed_package(self) -> None:
        tx = json.loads(Path(os.environ["REMOVE"]).read_text())
        self.assertEqual([], tx["install"])
        self.assertIn("foo-0:2-1.noarch", tx["remove"])
