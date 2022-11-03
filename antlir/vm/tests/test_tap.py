#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
from typing import Set

from antlir.tests.common import AntlirTestCase
from antlir.unshare import Namespace, Unshare
from antlir.vm.tap import TAPDEV, VmTap


class TestTap(AntlirTestCase):
    def get_links(self, ns: Unshare) -> Set[str]:
        links = (
            subprocess.run(
                ns.nsenter_as_user("ip", "link"),
                check=True,
                stdout=subprocess.PIPE,
                text=True,
            )
            .stdout.strip()
            .splitlines()
        )
        return {l.split(" ")[1].rstrip(":") for l in links}

    def test_create(self) -> None:
        with Unshare([Namespace.NETWORK]) as ns:
            before = self.get_links(ns)
            self.assertNotIn(TAPDEV, before)

            VmTap(netns=ns, uid=os.getuid(), gid=os.getgid())

            after = self.get_links(ns)
            self.assertIn(TAPDEV, after)
            self.assertEqual(len(after), len(before) + 1)

    def test_create_dev_net_tun(self) -> None:
        # remove /dev/net/tun so that the caller has to create it
        subprocess.run(["sudo", "rm", "-f", "/dev/net/tun"], check=True)
        subprocess.run(["sudo", "rm", "-rf", "/dev/net"], check=True)
        self.assertFalse(os.path.exists("/dev/net/tun"))
        self.assertFalse(os.path.exists("/dev/net"))
        self.test_create()
