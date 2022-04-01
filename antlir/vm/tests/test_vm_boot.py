#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import socket

from antlir.fs_utils import Path
from antlir.tests.common import AntlirTestCase


class BootVMTest(AntlirTestCase):
    def test_os_booted(self) -> None:
        self.assertTrue(Path("/etc/i_am_a_custom_rootfs").exists())

    def test_os_hostname(self) -> None:
        self.assertEqual(socket.gethostname(), "vmtest")
