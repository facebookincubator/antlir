#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest


class UnsanitizedEnvTest(unittest.TestCase):
    def test_env(self) -> None:
        self.assertTrue(
            # Comes from buck
            ("BUCK_BUILD_ID" in os.environ)
            # Comes from buck2
            or ("BUCK_WRAPPER_UUID" in os.environ)
        )
        # Comes from the test's `env`
        self.assertEqual("meow", os.environ["kitteh"])
