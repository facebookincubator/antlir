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
from antlir.vm.tap import VmTap


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
        tapdev = "vm0"
        with Unshare([Namespace.NETWORK]) as ns:
            before = self.get_links(ns)
            self.assertNotIn(tapdev, before)

            VmTap(netns=ns, uid=os.getuid(), gid=os.getgid(), index=0)

            after = self.get_links(ns)
            self.assertIn(tapdev, after)
            self.assertEqual(len(after), len(before) + 1)

    def test_create_dev_net_tun(self) -> None:
        # remove /dev/net/tun so that the caller has to create it
        subprocess.run(["sudo", "rm", "-f", "/dev/net/tun"], check=True)
        subprocess.run(["sudo", "rm", "-rf", "/dev/net"], check=True)
        self.assertFalse(os.path.exists("/dev/net/tun"))
        self.assertFalse(os.path.exists("/dev/net"))
        self.test_create()

    def test_mac_and_ip(self) -> None:
        with Unshare([Namespace.NETWORK]) as ns:
            eth0 = VmTap(netns=ns, uid=os.getuid(), gid=os.getgid(), index=0)
            self.assertEqual(eth0.guest_mac, "00:00:00:00:00:01")
            self.assertEqual(eth0.host_ipv6, "fd00::1/64")
            self.assertEqual(eth0.guest_ipv6, "fd00::2")

            eth3 = VmTap(netns=ns, uid=os.getuid(), gid=os.getgid(), index=3)
            self.assertEqual(eth3.guest_mac, "00:00:00:00:00:04")
            self.assertEqual(eth3.host_ipv6, "fd00:3::1/64")
            self.assertEqual(eth3.guest_ipv6, "fd00:3::2")
