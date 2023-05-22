#!/usr/bin/env fbpython
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os.path
import subprocess
import unittest

from antlir.config import repo_config

is_buck1 = os.environ.get("ANTLIR_BUCK", None) == "buck1"


class TestInstallFileStrip(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    @unittest.skipIf(
        is_buck1, "This doesn't work on buck1, and I refuse to spend time fixing it"
    )
    def test_gdb_loads_symbols(self) -> None:
        stdout = subprocess.run(
            ["gdb", "/usr/bin/test-cpp-binary", "-ex", "quit"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        if repo_config().artifacts_require_repo:
            self.assertIn(
                "Reading symbols from /usr/bin/test-cpp-binary...Reading symbols from /usr/lib/debug//usr/bin/test-cpp-binary.debug",
                stdout,
            )
        else:
            self.assertIn(
                "Reading symbols from /usr/bin/test-cpp-binary...Reading symbols from /usr/lib/debug/.build-id/",
                stdout,
            )

    @unittest.skipIf(
        repo_config().artifacts_require_repo,
        "size calculations are unreliable under dev mode",
    )
    def test_stripped_binary_is_smaller(self) -> None:
        stripped_size = os.path.getsize("/usr/bin/test-cpp-binary")
        full_size = os.path.getsize("/usr/bin/test-cpp-binary.full")
        self.assertLess(stripped_size, full_size / 2)
        symbols_size = os.path.getsize("/usr/lib/debug/usr/bin/test-cpp-binary.debug")
        self.assertAlmostEqual(
            stripped_size + symbols_size,
            full_size,
            # pyre-fixme[6]: For 3rd argument expected `None` but got `float`.
            delta=full_size * 0.05,
            msg="Expected stripped+symbols size to be within 5% of the full binary size, something smells off...",
        )
