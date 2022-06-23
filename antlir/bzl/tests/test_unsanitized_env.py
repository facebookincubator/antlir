#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest


class UnsanitizedEnvTest(unittest.TestCase):
    def test_env(self) -> None:
        if "IS_BUCK2" not in os.environ:
            self.assertTrue("BUCK_BUILD_ID" in os.environ)
        # Comes from the test's `env`
        self.assertEqual("meow", os.environ["kitteh"])
