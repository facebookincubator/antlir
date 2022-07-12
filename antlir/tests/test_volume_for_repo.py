#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import shutil
import subprocess
import tempfile
import unittest

from antlir import volume_for_repo as vfr

from antlir.fs_utils import Path


class VolumeForRepoTestCase(unittest.TestCase):
    def test_volume_repo(self) -> None:
        artifacts_dir = Path(tempfile.mkdtemp(prefix="test_volume_repo"))
        volume_dir = artifacts_dir / vfr.VOLUME_DIR
        image_file = artifacts_dir / vfr.IMAGE_FILE
        min_free_bytes = 250e6

        try:
            self.assertEqual(
                vfr.get_volume_for_current_repo(
                    artifacts_dir=artifacts_dir, min_free_bytes=min_free_bytes
                ),
                volume_dir,
            )
            self.assertGreaterEqual(os.stat(image_file).st_size, min_free_bytes)

            fstype_and_avail = subprocess.check_output(
                [
                    "findmnt",
                    "--noheadings",
                    "--output",
                    "FSTYPE,AVAIL",
                    "--bytes",
                    volume_dir,
                ]
            )
            fstype, avail = fstype_and_avail.strip().split()
            self.assertEqual(b"btrfs", fstype)
            self.assertGreaterEqual(int(avail), min_free_bytes)
        finally:
            try:
                subprocess.call(["sudo", "umount", volume_dir])
            except Exception:
                pass  # Might not have been mounted in case of an earlier error
            shutil.rmtree(artifacts_dir)


if __name__ == "__main__":
    unittest.main()
