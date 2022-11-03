#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest

from antlir.config import antlir_dep
from antlir.fs_utils import temp_dir
from antlir.rpm.find_snapshot import snapshot_install_dir
from antlir.tests.flavor_helpers import get_rpm_installers_supported


class ImageUnittestTestRepoServer(unittest.TestCase):
    def test_install_rpm(self) -> None:
        snapshot_dir = snapshot_install_dir(antlir_dep("rpm:repo-snapshot-for-tests"))
        for prog in get_rpm_installers_supported():
            with temp_dir() as td:
                os.mkdir(td / ".meta")
                subprocess.check_call(
                    [
                        snapshot_dir / prog / "bin" / prog,
                        f"--installroot={td}",
                        "install",
                        "--assumeyes",
                        "rpm-test-carrot",
                    ]
                )
                # We don't need a full rendered subvol test, since the
                # contents of the filesystem is checked by other tests.
                # (e.g.  `test-yum-dnf-from-snapshot`, `test-image-layer`)
                with open(td / "rpm_test/carrot.txt") as infile:
                    self.assertEqual("carrot 2 rc0\n", infile.read())
