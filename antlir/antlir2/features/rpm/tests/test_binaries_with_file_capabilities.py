# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import subprocess
import unittest


class TestBinariesWithFileCapabilities(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def test_newuidmap_caps(self) -> None:
        self.assertEqual(
            subprocess.run(
                ["getcap", "/usr/bin/antlir2-with-capability"],
                capture_output=True,
                text=True,
                check=True,
            ).stdout.strip(),
            "/usr/bin/antlir2-with-capability cap_setuid=ep",
        )
