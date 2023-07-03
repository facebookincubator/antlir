#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import re
import stat
import unittest

from antlir.fs_utils import Path
from antlir.tests.layer_resource import layer_resource_subvol


class TestExtracted(unittest.TestCase):
    # libsystemd-shared-*.so is only found in the binary's RPATH, not in /lib64
    def test_rpath(self):
        subvol = layer_resource_subvol(__package__, "layer")
        paths = Path.listdir(subvol.path("/usr/lib64/systemd"))
        self.assertTrue(
            any(
                re.match(rb"libsystemd-shared-\d+.*\.so", path.basename())
                for path in paths
            ),
            "libsystemd-shared does not appear to be in {}".format(paths),
        )

    # the interpreter is under /lib64, but we want to clone it to /usr/lib64
    # when /lib64 is a symlink (which should be always for the cases that we
    # care about)
    def test_cloned_to_usr(self):
        # ensure that the source for the extractor actually has the symlink
        # setup
        source_subvol = layer_resource_subvol(__package__, "source")
        self.assertTrue(source_subvol.path("/lib64").islink())
        self.assertEqual(source_subvol.path("/lib64").readlink(), Path("usr/lib64"))

        subvol = layer_resource_subvol(__package__, "layer")
        self.assertEqual(subvol.path("/lib64").readlink(), Path("usr/lib64"))
        self.assertTrue(subvol.path("/usr/lib64/libc.so.6"))

    def test_permissions(self):
        src_subvol = layer_resource_subvol(__package__, "source")
        dst_subvol = layer_resource_subvol(__package__, "layer")
        for path in ("/usr/lib", "/usr/bin"):
            with self.subTest(path):
                src = os.stat(src_subvol.path(path))
                dst = os.stat(dst_subvol.path(path))
                self.assertEqual(stat.filemode(src.st_mode), stat.filemode(dst.st_mode))

    def test_binaries_run(self):
        subvol = layer_resource_subvol(__package__, "layer")
        # repo built binary
        subvol.run_as_root([subvol.path("/usr/bin/repo-built-binary")], check=True)

        # binary from rpms
        for binary in (
            "/usr/lib/systemd/systemd",
            "/usr/bin/strace",
        ):
            # Note: we have to run this via chroot so that the shared libs
            # are properly loaded from within the image.layer and _not_ the
            # host environment.  We don't use systemd-nspawn here because it
            # requires a "real" os that has a passwd db and everything.
            subvol.run_as_root(["chroot", subvol.path(), binary, "--help"], check=True)
