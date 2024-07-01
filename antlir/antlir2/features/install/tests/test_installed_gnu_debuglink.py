#!/usr/bin/env fbpython
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os.path
import re
import subprocess
import unittest


def skip_in_dev(f):
    """
    No-op this test in dev mode builds. An actual Skip is reported as more
    angry-looking in tpx so instead just don't even define this function
    """
    if os.path.islink("/foo/true-rs"):
        return
    return f


class TestInstalledBinaryGnuDebuglink(unittest.TestCase):
    @skip_in_dev
    def test_gnu_debuglink_used(self):
        subprocess.run(["/foo/true-rs"], check=True)
        self.assertCountEqual(
            [
                "true-rs",
                "true-rs.debug",
                "true-rs.debug.dwp",
            ],
            os.listdir("/foo"),
        )
        readelf_proc = subprocess.run(
            ["readelf", "--string-dump=.gnu_debuglink", "/foo/true-rs", "-wk"],
            text=True,
            capture_output=True,
            check=True,
            errors="ignore",
        )
        match = re.search(r"Separate debug info file: (.+)", readelf_proc.stdout)
        if not match or not match.group(1):
            self.fail(
                f"Could not find debuglink in binary: {readelf_proc.stdout}\nstderr: {readelf_proc.stderr}"
            )
        self.assertEqual(match.group(1), "true-rs.debug")
