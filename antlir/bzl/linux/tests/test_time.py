#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest


class TimeTest(unittest.TestCase):
    def test_timezome(self) -> None:
        """Verify that the timezone is set properly."""
        out = subprocess.run(
            ["/usr/bin/date", "+%Z"], check=True, stdout=subprocess.PIPE
        )
        self.assertIn(
            out.stdout.decode().strip(),
            os.environ["ANTLIR_TEST_EXPECTED_TIMEZONES"].split(),
        )
