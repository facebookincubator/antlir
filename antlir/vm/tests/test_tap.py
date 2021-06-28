#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest
from typing import Set

from antlir.unshare import Namespace, Unshare
from antlir.vm.tap import TAPDEV, VmTap


class TestTap(unittest.TestCase):
    def get_links(self, ns: Unshare) -> Set[str]:
        links = (
            subprocess.run(
                # pyre-fixme[6]: Expected `List[Variable[typing.AnyStr <: [str,
                #  bytes]]]` for 1st param but got `str`.
                ns.nsenter_as_user("ip", "link"),
                check=True,
                stdout=subprocess.PIPE,
                text=True,
            )
            .stdout.strip()
            .splitlines()
        )
        return {l.split(" ")[1].rstrip(":") for l in links}

    def test_create(self):
        with Unshare([Namespace.NETWORK]) as ns:
            before = self.get_links(ns)
            self.assertNotIn(TAPDEV, before)

            VmTap(netns=ns, uid=os.getuid(), gid=os.getgid())

            after = self.get_links(ns)
            self.assertIn(TAPDEV, after)
            self.assertEqual(len(after), len(before) + 1)

    def test_create_dev_net_tun(self):
        # remove /dev/net/tun so that the caller has to create it
        subprocess.run(["sudo", "rm", "/dev/net/tun"], check=True)
        subprocess.run(["sudo", "rm", "-rf", "/dev/net"], check=True)
        self.test_create()
