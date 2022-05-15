#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import contextlib
import subprocess
import unittest
import unittest.mock

from antlir.common import run_stdout_to_err
from antlir.fs_utils import Path, temp_dir
from antlir.loopback import (
    BtrfsLoopbackVolume,
    LoopbackVolume,
    MiB,
    MIN_CREATE_BYTES,
    MIN_SHRINK_BYTES,
)
from antlir.unshare import Namespace, Unshare


class LoopbackTestCases(unittest.TestCase):
    # All these tests make mounts, so to avoid leaking them we run in a mount
    # namespace. Moreover, we don't want to leak `sudo`ed commands on crash,
    # so use a PID namespace to ensure they get garbage-collected.

    @contextlib.contextmanager
    def _test_workspace(self):
        with Unshare([Namespace.MOUNT, Namespace.PID]) as ns, temp_dir() as td:
            yield (ns, td)

    def test_loopback(self) -> None:
        with self._test_workspace() as (ns, td):
            image_path = td / "image.btrfs"
            test_message = "I am a beautiful loopback"

            # Make a btrfs loopback
            with BtrfsLoopbackVolume(
                unshare=ns,
                image_path=image_path,
                size_bytes=128 * MiB,
                compression_level=1,
            ) as vol:
                self.assertEqual(128 * MiB, vol.get_size())
                cmd = f"echo '{test_message}' > {vol.dir()}/msg"
                run_stdout_to_err(
                    ns.nsenter_as_root(
                        "/bin/bash",
                        "-uec",
                        cmd,
                    ),
                )

            # Mount it as a generic LoopbackVolume
            # and confirm the contents
            with LoopbackVolume(
                unshare=ns,
                image_path=image_path,
                fs_type="btrfs",
            ) as vol:
                msg_file = vol.dir() / "msg"
                msg_text = subprocess.run(
                    ns.nsenter_as_root(
                        "cat",
                        msg_file,
                    ),
                    text=True,
                    capture_output=True,
                ).stdout.strip("\n")

                self.assertEqual(test_message, msg_text)

    @unittest.mock.patch("antlir.loopback.kernel_version")
    def test_btrfs_loopback_rounded_size(self, kernel_version) -> None:
        # Mock a kernel version that requires the size to be
        # rounded up.
        kernel_version.return_value = (4, 6)

        with self._test_workspace() as (ns, td):
            image_path = td / "image.btrfs"
            with BtrfsLoopbackVolume(
                unshare=ns,
                image_path=image_path,
                # We want to make this a non-multiple of 4096
                size_bytes=128 * MiB - 3,
                compression_level=1,
            ) as vol:
                # Confirm it has been rounded up
                self.assertEqual(128 * MiB, vol.get_size())

    def test_btrfs_loopback_min_create_size(self) -> None:
        with self._test_workspace() as (ns, td):
            image_path = td / "image.btrfs"

            # Make a btrfs loopback that is smaller than the min
            with self.assertRaisesRegex(
                AttributeError,
                f"A btrfs loopback must be at least {MIN_CREATE_BYTES}",
            ):
                with BtrfsLoopbackVolume(
                    unshare=ns,
                    image_path=image_path,
                    size_bytes=32768,  # Pretty small
                    compression_level=1,
                ):
                    pass

    def test_btrfs_loopback_minimize(self) -> None:
        # Make a btrfs loopback that is smaller than the min
        # shrink size to confirm that we don't shrink
        size = MIN_SHRINK_BYTES - (1 * MiB)
        with self._test_workspace() as (ns, td):
            image_path = td / "image.btrfs"

            with BtrfsLoopbackVolume(
                unshare=ns,
                image_path=image_path,
                size_bytes=size,
                compression_level=1,
            ) as vol:
                self.assertEqual(size, vol.get_size())
                self.assertEqual(size, vol.minimize_size())

        # Make a btrfs loopback that slightly larger
        # than the min shrink size to confirm that we shrink
        size = MIN_SHRINK_BYTES + (1 * MiB)
        with self._test_workspace() as (ns, td):
            image_path = td / "image.btrfs"

            with BtrfsLoopbackVolume(
                unshare=ns,
                image_path=image_path,
                size_bytes=size,
                compression_level=1,
            ) as vol:
                self.assertEqual(size, vol.get_size())
                self.assertEqual(MIN_SHRINK_BYTES, vol.minimize_size())

    def test_btrfs_loopback_receive(self) -> None:
        with Unshare([Namespace.MOUNT, Namespace.PID]) as ns, temp_dir() as td:
            image_path = td / "image.btrfs"

            with BtrfsLoopbackVolume(
                unshare=ns,
                image_path=image_path,
                size_bytes=MIN_CREATE_BYTES,
                compression_level=1,
            ) as vol, open(
                Path(__file__).dirname() / "create_ops.sendstream"
            ) as f:
                # pyre-fixme[6]: For 1st param expected `int` but got `TextIOWrapper`.
                ret = vol.receive(f)
                self.assertEqual(0, ret.returncode)
                self.assertIn(b"At subvol create_ops", ret.stderr)
