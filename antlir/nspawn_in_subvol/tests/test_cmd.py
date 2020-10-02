#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from ..cmd import _parse_cgroup_path


class CmdTestCase(unittest.TestCase):
    def test_parse_cgroup_path(self):
        # usually there is only this one line
        proc_self_cgroup = b"0::/user.slice/foo.slice/bar.scope\n"
        self.assertEqual(
            _parse_cgroup_path(proc_self_cgroup),
            b"/user.slice/foo.slice/bar.scope",
        )
        # sometimes there is an extra systemd hierarchy that we should ignore
        proc_self_cgroup = b"1:name=systemd:/\n" + proc_self_cgroup
        self.assertEqual(
            _parse_cgroup_path(proc_self_cgroup),
            b"/user.slice/foo.slice/bar.scope",
        )

        proc_self_cgroup += b"0::/invalid/second/match.scope\n"
        with self.assertRaises(AssertionError):
            _parse_cgroup_path(proc_self_cgroup)
