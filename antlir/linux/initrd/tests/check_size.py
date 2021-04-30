#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest

from antlir.config import load_repo_config


class CheckSize(unittest.TestCase):
    def test_package_size(self):
        cfg = load_repo_config()
        if cfg.artifacts_require_repo:
            self.skipTest(
                "package size only is only accurate in release builds "
                "with standalone artifacts"
            )

        package = os.environ["BASE_PACKAGE"]
        package_size = os.path.getsize(package)
        # ensure package does not unexpectedly grow
        self.assertLessEqual(package_size, 10 * 1024 * 1024)
        # incentive to make sure this test gets updated with any size wins
        self.assertGreaterEqual(package_size, 8.5 * 1024 * 1024)
