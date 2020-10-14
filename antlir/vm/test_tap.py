#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import contextlib
import os
import subprocess
import unittest
from typing import Set

from antlir.unshare import Namespace, Unshare
from antlir.vm.tap import TAPDEV, VmTap


class TestTap(unittest.TestCase):
    def setUp(self):
        with contextlib.ExitStack() as stack:
            self.ns = stack.enter_context(Unshare([Namespace.NETWORK]))
            self.addCleanup(stack.pop_all().close)

    def get_links(self) -> Set[str]:
        links = (
            subprocess.run(
                self.ns.nsenter_as_user("ip", "link"),
                check=True,
                stdout=subprocess.PIPE,
                text=True,
            )
            .stdout.strip()
            .splitlines()
        )
        return {l.split(" ")[1].rstrip(":") for l in links}

    def test_create(self):
        before = self.get_links()
        self.assertNotIn(TAPDEV, before)

        VmTap(netns=self.ns, uid=os.getuid(), gid=os.getgid())

        after = self.get_links()
        self.assertIn(TAPDEV, after)
        self.assertEqual(len(after), len(before) + 1)
