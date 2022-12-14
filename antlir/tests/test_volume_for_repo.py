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

from antlir import btrfsutil, volume_for_repo as vfr

from antlir.fs_utils import Path


class VolumeForRepoTestCase(unittest.TestCase):
    def test_volume_repo(self) -> None:
        artifacts_dir = Path(tempfile.mkdtemp(prefix="test_volume_repo"))
        volume_dir = artifacts_dir / vfr.VOLUME_DIR

        try:
            self.assertEqual(
                vfr.get_volume_for_current_repo(
                    artifacts_dir=artifacts_dir,
                ),
                volume_dir,
            )

            self.assertTrue(btrfsutil.is_subvolume(volume_dir))
        finally:
            try:
                btrfsutil.delete_subvolume(volume_dir, recursive=True)
            except Exception:
                pass  # Might not have been created in case of an earlier error
            shutil.rmtree(artifacts_dir)

    def test_upgrade(self):
        """
        Can upgrade from loopback to on-host subvol
        """
        artifacts_dir = Path(tempfile.mkdtemp(prefix="test_upgrade_"))
        volume_dir = artifacts_dir / vfr.VOLUME_DIR
        artifacts_dir_src = Path(tempfile.mkdtemp(prefix="test_upgrade_src_"))
        os.makedirs(volume_dir)
        subprocess.run(
            ["sudo", "mount", "--bind", artifacts_dir_src, volume_dir], check=True
        )

        try:
            self.assertEqual(
                vfr.get_volume_for_current_repo(
                    artifacts_dir=artifacts_dir,
                ),
                volume_dir,
            )
            self.assertTrue(btrfsutil.is_subvolume(volume_dir))
        finally:
            try:
                btrfsutil.delete_subvolume(volume_dir, recursive=True)
            except Exception:
                pass  # Might not have been created in case of an earlier error
            shutil.rmtree(artifacts_dir)
            shutil.rmtree(artifacts_dir_src)


if __name__ == "__main__":
    unittest.main()
