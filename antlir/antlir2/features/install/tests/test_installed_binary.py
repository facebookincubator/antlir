#!/usr/bin/env fbpython
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os.path
import subprocess
import unittest


def skip_in_dev(f):
    """
    No-op this test in dev mode builds. An actual Skip is reported as more
    angry-looking in tpx so instead just don't even define this function
    """
    if os.path.islink("/usr/bin/true-rs"):
        return
    return f


class TestInstalledBinary(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()

    def test_runs(self) -> None:
        for lang in ["rs", "py"]:
            with self.subTest(lang):
                subprocess.run([f"true-{lang}"], check=True)

    @skip_in_dev
    def test_gdb_loads_symbols(self) -> None:
        stdout = subprocess.run(
            ["gdb", "true-rs", "-ex", "quit"],
            capture_output=True,
            text=True,
            check=True,
        ).stdout
        self.assertIn(
            "Reading symbols from true-rs...\nReading symbols from /usr/lib/debug/.build-id/",
            stdout,
        )

    @skip_in_dev
    def test_stripped_binary_is_smaller(self) -> None:
        stripped_size = os.path.getsize("/usr/bin/true-rs")
        unstripped_size = os.path.getsize("/usr/bin/true-rs.unstripped")
        self.assertLess(stripped_size, unstripped_size)
