#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import subprocess
import unittest

from antlir.fs_utils import Path


# This is separate from `test_fs_utils.py` so we can run it for all
# supported Python package formats.
class TestFsUtilsPathResource(unittest.TestCase):
    def test_path_resource(self) -> None:
        # Thanks to `exe=True`, we should be able to run the resource.
        with Path.resource(__package__, "helper-binary", exe=True) as p:
            self.assertEqual(
                "I am SO helpful.\n", subprocess.check_output([p], text=True)
            )
        # With `exe=False`, we can still read the resource. There's not
        # much to assert about the contents, since it may be compressed,
        # and need not contain the literal magic string.
        with Path.resource(__package__, "helper-binary", exe=False) as p, open(
            p, "rb"
        ) as f:
            self.assertLess(10, len(f.read()))
