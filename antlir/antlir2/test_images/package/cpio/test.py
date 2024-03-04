# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import importlib.resources
import subprocess
import unittest
from dataclasses import dataclass


@dataclass(frozen=True, order=True)
class Entry(object):
    name: str
    mode: str
    user: str
    group: str
    size: int


class Test(unittest.TestCase):
    def test_cpio(self) -> None:
        with importlib.resources.open_binary(__package__, "test.cpio") as f:
            proc = subprocess.run(
                ["cpio", "--list", "--verbose"],
                stdin=f,
                check=True,
                text=True,
                capture_output=True,
            )

        # remove the timestamps since that's not stable
        entries = [line.split() for line in proc.stdout.splitlines()]
        entries = sorted(
            [
                Entry(mode=e[0], user=e[2], group=e[3], size=int(e[4]), name=e[-1])
                for e in entries
            ]
        )
        self.assertEqual(
            entries,
            [
                Entry(
                    name=".meta", mode="drwxr-xr-x", user="root", group="root", size=0
                ),
                Entry(
                    name=".meta/target",
                    mode="-rw-r--r--",
                    user="root",
                    group="root",
                    size=58,
                ),
                Entry(
                    name="/foo", mode="lrwxrwxrwx", user="root", group="root", size=4
                ),
                Entry(name="foo", mode="-r--r--r--", user="root", group="root", size=4),
            ],
        )
