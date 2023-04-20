#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

from subprocess import PIPE, run

from antlir.fs_utils import Path
from antlir.tests.common import AntlirTestCase


SYSTEMCTL_BIN = "/usr/bin/systemctl"


class MultiNicVMTest(AntlirTestCase):
    def test_networkd_wait_ran(self) -> None:
        """Test we have booted the VM and ran systemd-networkd-wait-online service"""
        systemctl_run = run(
            [SYSTEMCTL_BIN, "status", "systemd-networkd-wait-online.service"],
            stdout=PIPE,
            check=True,
            encoding="utf8",
        )
        self.assertTrue("Active: active (exited)" in systemctl_run.stdout)

    def test_eth2_exists(self) -> None:
        self.assertTrue(Path("/sys/class/net/eth2/address").exists())
