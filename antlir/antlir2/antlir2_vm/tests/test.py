#!/usr/bin/env fbpython
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest


class TestInstalledBinary(unittest.TestCase):
    def test_is_root(self) -> None:
        self.assertEqual(0, os.getuid())

    def test_env_propagated(self) -> None:
        self.assertEqual("1", os.getenv("ANTLIR2_TEST"))

    def test_env_artifact_exists(self) -> None:
        artifact = os.getenv("ENV_ARTIFACT")
        self.assertNotEqual(None, artifact)
        self.assertTrue(os.path.exists(artifact))
