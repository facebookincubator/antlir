#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from unittest import mock
from unittest.mock import mock_open, patch

from antlir.nspawn_in_subvol.common import (
    find_cgroup2_mountpoint,
    nspawn_version,
    NSpawnVersion,
    parse_cgroup2_path,
)


class CommonTestCase(unittest.TestCase):
    def test_nspawn_version(self) -> None:
        with mock.patch("subprocess.check_output") as version:
            version.return_value = (
                "systemd 602214076 (v602214076-2.fb1)\n+AVOGADROS SYSTEMD\n"
            )
            self.assertEqual(
                NSpawnVersion(major=602214076, full="602214076-2.fb1"),
                nspawn_version(),
            )

        # Check that the real nspawn on the machine running this test is
        # actually a sane version.  We need at least 239 to do anything useful
        # and 1000 seems like a reasonable upper bound, but mostly I'm just
        # guessing here.
        self.assertTrue(nspawn_version().major > 239)
        self.assertTrue(nspawn_version().major < 1000)

    def test_arch_version(self) -> None:
        # the above version check unit test is biased towards an fb environment
        # systemd in other distributions can have different version formats
        with mock.patch("subprocess.check_output") as version:
            version.return_value = (
                "systemd 246 (246.4-1-arch)\n"
                "+PAM +AUDIT -SELINUX -IMA -APPARMOR +SMACK -SYSVINIT "
                "+UTMP +LIBCRYPTSETUP +GCRYPT +GNUTLS +ACL +XZ +LZ4 "
                "+ZSTD +SECCOMP +BLKID +ELFUTILS +KMOD +IDN2 -IDN "
                "+PCRE2 default-hierarchy=hybrid"
            )
            self.assertEqual(
                NSpawnVersion(major=246, full="246.4-1-arch"), nspawn_version()
            )

    def test_cgroup2_mountpoint_usual(self) -> None:
        with patch(
            "antlir.nspawn_in_subvol.common._open_mounts",
            mock_open(
                read_data=b"cgroup2 /sys/fs/cgroup cgroup2 "
                b"rw,nosuid,nodev,noexec,relatime 0 0\n"
                b"/dev/mapper/ssd-ssdstripe / btrfs "
                b"rw,relatime,compress=zstd:3,ssd,space_cache,subvolid=5,"
                b"subvol=/ 0 0\n"
            ),
        ):
            self.assertEqual(b"/sys/fs/cgroup", find_cgroup2_mountpoint())

    def test_cgroup2_mountpoint_unified(self) -> None:
        with patch(
            "antlir.nspawn_in_subvol.common._open_mounts",
            mock_open(
                read_data=b"cgroup2 /sys/fs/cgroup/unified cgroup2 "
                b"rw,nosuid,nodev,noexec,relatime 0 0\n"
                b"/dev/mapper/ssd-ssdstripe / btrfs "
                b"rw,relatime,compress=zstd:3,ssd,space_cache,subvolid=5,"
                b"subvol=/ 0 0\n"
            ),
        ):
            self.assertEqual(b"/sys/fs/cgroup/unified", find_cgroup2_mountpoint())

    def test_cgroup2_custom_fs_spec(self) -> None:
        with patch(
            "antlir.nspawn_in_subvol.common._open_mounts",
            mock_open(
                read_data=b"your_mamas_cgroup /sys/fs/cgroup cgroup2 "
                b"rw,nosuid,nodev,noexec,relatime 0 0\n"
                b"/dev/mapper/ssd-ssdstripe / btrfs "
                b"rw,relatime,compress=zstd:3,ssd,space_cache,subvolid=5,"
                b"subvol=/ 0 0\n"
            ),
        ):
            self.assertEqual(b"/sys/fs/cgroup", find_cgroup2_mountpoint())

    def test_cgroup2_no_mountpoint_found(self) -> None:
        with patch(
            "antlir.nspawn_in_subvol.common._open_mounts",
            mock_open(
                read_data=b"devtmpfs /dev devtmpfs "
                b"rw,nosuid,size=4096k,nr_inodes=65536,mode=755 0 0\n"
                b"/dev/mapper/ssd-ssdstripe / btrfs "
                b"rw,relatime,compress=zstd:3,ssd,space_cache,subvolid=5,"
                b"subvol=/ 0 0\n"
            ),
        ):
            with self.assertRaisesRegex(RuntimeError, "No cgroupv2 mountpoint found"):
                find_cgroup2_mountpoint()

    def test_parse_cgroup_path(self) -> None:
        # usually there is only this one line
        proc_self_cgroup = b"0::/user.slice/foo.slice/bar.scope\n"
        self.assertEqual(
            parse_cgroup2_path(proc_self_cgroup),
            b"/user.slice/foo.slice/bar.scope",
        )
        # sometimes there is an extra systemd hierarchy that we should ignore
        proc_self_cgroup = b"1:name=systemd:/\n" + proc_self_cgroup
        self.assertEqual(
            parse_cgroup2_path(proc_self_cgroup),
            b"/user.slice/foo.slice/bar.scope",
        )

        proc_self_cgroup += b"0::/invalid/second/match.scope\n"
        with self.assertRaises(AssertionError):
            parse_cgroup2_path(proc_self_cgroup)
