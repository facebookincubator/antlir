#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import shutil
import subprocess
import tempfile
import time
import unittest

from antlir import btrfsutil, volume_for_repo as vfr
from antlir.artifacts_dir import find_repo_root

from antlir.errors import InfraError

from antlir.fs_utils import Path


class VolumeForRepoTestCase(unittest.TestCase):
    def _scratch_subvolume(self) -> str:
        """
        Crate a scratch subvolume on a BTRFS filesystem where we can create
        pathological test cases for volume_for_repo.py. We can't use existing
        Antlir mechanisms like TempSubvolumes, because volume_for_repo is the
        thing that sets up the environment for those to work.
        """
        repo_root = find_repo_root()
        # hopefully the parent of the repo is on a btrfs filesystem
        hopefully_btrfs = repo_root.dirname()
        scratch_path = (
            hopefully_btrfs / f"test_volume_for_repo_{self.id()}_{time.time()}"
        )
        self.addCleanup(
            lambda scratch_path: btrfsutil.delete_subvolume(
                scratch_path, recursive=True
            ),
            scratch_path,
        )
        btrfsutil.create_subvolume(scratch_path)
        subprocess.run(
            ["sudo", "chown", f"{os.getuid()}:{os.getgid()}", scratch_path], check=True
        )
        return str(scratch_path)

    def test_volume_repo(self) -> None:
        artifacts_dir = Path(
            tempfile.mkdtemp(prefix=self.id(), dir=self._scratch_subvolume())
        )
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

    def test_exists_but_not_subvolume_and_not_empty(self):
        """
        Existing volume dir that is not a subvolume but is not empty should ask
        the user to delete the contents themselves just in case.
        """
        artifacts_dir = Path(tempfile.mkdtemp(prefix=self.id()))
        volume_dir = artifacts_dir / vfr.VOLUME_DIR
        os.mkdir(volume_dir)
        (volume_dir / "file").touch()
        with self.assertRaisesRegex(InfraError, "is not a subvolume"):
            vfr.get_volume_for_current_repo(artifacts_dir=artifacts_dir)

    def test_is_subvolid5(self):
        """
        Something that is already btrfs but is subvolid=5, indicating that it's
        a loopback. When not in use, this will cleanly unmount.
        """
        artifacts_dir = Path(
            tempfile.mkdtemp(prefix=self.id(), dir=self._scratch_subvolume())
        )
        volume_dir = artifacts_dir / vfr.VOLUME_DIR
        os.mkdir(volume_dir)
        with tempfile.NamedTemporaryFile() as loopback:
            loopback.truncate(500 * 1024 * 1024)
            subprocess.run(["mkfs.btrfs", loopback.name], check=True)
            subprocess.run(["sudo", "mount", loopback.name, volume_dir], check=True)
            self.assertEqual(btrfsutil.subvolume_id(volume_dir), 5)
            self.assertEqual(
                vfr.get_volume_for_current_repo(
                    artifacts_dir=artifacts_dir,
                ),
                volume_dir,
            )
            self.assertTrue(btrfsutil.is_subvolume(volume_dir))
            self.assertNotEqual(btrfsutil.subvolume_id(volume_dir), 5)

    def test_subvolid5_with_contents_underneath(self):
        """
        Something that is already btrfs but is subvolid=5, indicating that it's
        a loopback. When not in use, this will cleanly unmount. However, if
        there are contents that were being shadowed by the mount, we ask the
        user to remove them.
        """
        artifacts_dir = Path(
            tempfile.mkdtemp(prefix=self.id(), dir=self._scratch_subvolume())
        )
        volume_dir = artifacts_dir / vfr.VOLUME_DIR
        os.mkdir(volume_dir)
        (volume_dir / "file").touch()
        with tempfile.NamedTemporaryFile() as loopback:
            loopback.truncate(500 * 1024 * 1024)
            subprocess.run(["mkfs.btrfs", loopback.name], check=True)
            subprocess.run(["sudo", "mount", loopback.name, volume_dir], check=True)
            self.assertEqual(btrfsutil.subvolume_id(volume_dir), 5)
            with self.assertRaisesRegex(InfraError, "could not be removed"):
                vfr.get_volume_for_current_repo(
                    artifacts_dir=artifacts_dir,
                )

    def test_is_subvolid5_and_in_use(self):
        """
        Something that is already btrfs but is subvolid=5, indicating that it's
        a loopback. We will try to clean this up automatically, but require
        manual intervention if it could not be unmounted.
        """
        artifacts_dir = Path(tempfile.mkdtemp(prefix=self.id()))
        volume_dir = artifacts_dir / vfr.VOLUME_DIR
        os.mkdir(volume_dir)
        with tempfile.NamedTemporaryFile() as loopback:
            loopback.truncate(500 * 1024 * 1024)
            subprocess.run(["mkfs.btrfs", loopback.name], check=True)
            subprocess.run(["sudo", "mount", loopback.name, volume_dir], check=True)
            subprocess.run(
                [
                    "sudo",
                    "chown",
                    f"{os.getuid()}:{os.geteuid()}",
                    loopback.name,
                    volume_dir,
                ],
                check=True,
            )
            with open(volume_dir / "force-in-use", "w") as f:
                f.write("foo")
                with self.assertRaisesRegex(
                    InfraError, "appears to be a mounted btrfs"
                ):
                    vfr.get_volume_for_current_repo(artifacts_dir=artifacts_dir)
            subprocess.run(["sudo", "umount", volume_dir])


if __name__ == "__main__":
    unittest.main()
