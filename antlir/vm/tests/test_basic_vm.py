#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import multiprocessing
import os
import platform

from antlir.fs_utils import Path
from antlir.tests.common import AntlirTestCase


class BasicVMTest(AntlirTestCase):
    def test_env(self) -> None:
        self.assertEqual(os.environ.pop("kitteh"), "meow")
        self.assertEqual(os.environ.pop("dogsgo"), "woof")

    def test_load_resource(self) -> None:
        with importlib.resources.path(__package__, "resource") as path:
            with open(path, "r") as f:
                self.assertEqual(
                    f.read(),
                    "There is nothing either good or bad, "
                    "but thinking makes it so.\n",
                )

    def test_running_as_root(self) -> None:
        self.assertEqual(os.getuid(), 0)

    def test_rootfs_is_writable(self) -> None:
        with open("/set_us_up", "w") as f:
            f.write("All your base are belong to us!\n")

        with open("/set_us_up", "r") as f:
            self.assertEqual(f.read(), "All your base are belong to us!\n")

    def test_running_multiple_cpus(self) -> None:
        self.assertEqual(multiprocessing.cpu_count(), 4)

    def test_custom_rootfs(self) -> None:
        self.assertTrue(os.path.exists("/etc/i_am_a_custom_rootfs"))

    def test_kernel_modules(self) -> None:
        modules_path = Path("/usr/lib/modules") / platform.uname().release
        self.assertTrue(modules_path.exists())
        self.assertTrue(os.path.ismount(modules_path))

    def test_eth0_exists(self) -> None:
        self.assertTrue(Path("/sys/class/net/eth0/address").exists())
