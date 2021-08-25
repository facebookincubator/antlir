#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import multiprocessing
import os
import platform
import socket
import unittest

from antlir.fs_utils import Path
from antlir.tests.common import AntlirTestCase


class BasicVMTest(AntlirTestCase):
    def test_env(self):
        self.assertEqual(os.environ.pop("kitteh"), "meow")
        self.assertEqual(os.environ.pop("dogsgo"), "woof")

    def test_running_in_vm(self):
        self.assertEqual(socket.gethostname(), "vmtest")

    def test_load_resource(self):
        with importlib.resources.path(__package__, "resource") as path:
            with open(path, "r") as f:
                self.assertEqual(
                    f.read(),
                    "There is nothing either good or bad, "
                    "but thinking makes it so.\n",
                )

    def test_running_as_root(self):
        self.assertEqual(os.getuid(), 0)

    def test_rootfs_is_writable(self):
        with open("/set_us_up", "w") as f:
            f.write("All your base are belong to us!\n")

        with open("/set_us_up", "r") as f:
            self.assertEqual(f.read(), "All your base are belong to us!\n")

    def test_running_multiple_cpus(self):
        self.assertEqual(multiprocessing.cpu_count(), 4)

    def test_custom_rootfs(self):
        self.assertTrue(os.path.exists("/etc/i_am_a_custom_rootfs"))

    def test_kernel_modules(self):
        modules_path = Path("/usr/lib/modules") / platform.uname().release
        self.assertTrue(modules_path.exists())
        self.assertTrue(os.path.ismount(modules_path))
