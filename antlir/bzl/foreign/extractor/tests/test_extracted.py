#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import re
import unittest

from antlir.fs_utils import Path
from antlir.tests.layer_resource import layer_resource_subvol


class TestExtracted(unittest.TestCase):
    # libsystemd-shared-*.so is only found in the binary's RPATH, not in /lib64
    def test_rpath(self):
        subvol = layer_resource_subvol(__package__, "layer")
        paths = Path.listdir(subvol.path("/usr/lib/systemd"))
        self.assertTrue(
            any(
                re.match(rb"libsystemd-shared-\d+\.so", path.basename())
                for path in paths
            )
        )

    # the interpreter is under /lib64, but we want to clone it to /usr/lib64
    # when /lib64 is a symlink (which should be always for the cases that we
    # care about)
    def test_cloned_to_usr(self):
        # ensure that the source for the extractor actually has the symlink setup
        source_subvol = layer_resource_subvol(__package__, "source")
        self.assertTrue(source_subvol.path("/lib64").islink())
        self.assertEqual(
            source_subvol.path("/lib64").readlink(), Path("usr/lib64")
        )

        subvol = layer_resource_subvol(__package__, "layer")
        self.assertFalse(subvol.path("/lib64").exists())
        self.assertTrue(subvol.path("/usr/lib64/libc.so.6"))
