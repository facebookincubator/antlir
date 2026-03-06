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

    @skip_in_dev
    def test_strip_all_binary_is_smaller_than_stripped(self) -> None:
        strip_all_size = os.path.getsize("/usr/bin/true-rs.strip-all")
        stripped_size = os.path.getsize("/usr/bin/true-rs")
        unstripped_size = os.path.getsize("/usr/bin/true-rs.unstripped")
        # strip_all should be smaller than both default-stripped and unstripped
        self.assertLess(strip_all_size, unstripped_size)
        self.assertLess(strip_all_size, stripped_size)

    @skip_in_dev
    def test_strip_all_binary_runs(self) -> None:
        subprocess.run(["/usr/bin/true-rs.strip-all"], check=True)

    @skip_in_dev
    def test_strip_all_has_no_symtab(self) -> None:
        result = subprocess.run(
            ["readelf", "-S", "/usr/bin/true-rs.strip-all"],
            capture_output=True,
            text=True,
            check=True,
        )
        self.assertNotIn(".symtab", result.stdout)

    @skip_in_dev
    def test_strip_all_gdb_loads_debuginfo(self) -> None:
        """Verify that gdb can resolve symbols from separate debuginfo for a
        strip_all binary. The binary itself has no .symtab, but gdb should
        find the main function via the build-id debuginfo in /usr/lib/debug."""
        result = subprocess.run(
            [
                "gdb",
                "/usr/bin/true-rs.strip-all",
                "-batch",
                "-ex",
                "info address main",
            ],
            capture_output=True,
            text=True,
            check=True,
        )
        # gdb should resolve 'main' from the separate debuginfo and print its
        # address, e.g. 'Symbol "main" is at 0x... in a file compiled ...'
        self.assertIn('Symbol "main"', result.stdout)

    @skip_in_dev
    def test_outplace_par(self) -> None:
        self.assertTrue(
            os.path.isdir(
                "/usr/local/libexec/python_outplace/antlir_antlir2_features_install_tests/true-py-outplace#link-tree"
            )
        )
        self.assertTrue(os.path.islink("/usr/bin/true-py"))
        self.assertEqual(
            os.path.realpath("/usr/bin/true-py"),
            "/usr/local/libexec/python_outplace/antlir_antlir2_features_install_tests/true-py-outplace#par",
        )
