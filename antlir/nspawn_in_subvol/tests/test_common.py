#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest
from unittest import mock

from ..common import NSpawnVersion, nspawn_version


class CommonTestCase(unittest.TestCase):
    def test_nspawn_version(self):
        with mock.patch("subprocess.check_output") as version:
            version.return_value = (
                "systemd 602214076 (v602214076-2.fb1)\n+AVOGADROS SYSTEMD\n"
            )
            self.assertEqual(
                NSpawnVersion(major=602214076, full="v602214076-2.fb1"),
                nspawn_version(),
            )

        # Check that the real nspawn on the machine running this test is
        # actually a sane version.  We need at least 239 to do anything useful
        # and 1000 seems like a reasonable upper bound, but mostly I'm just
        # guessing here.
        self.assertTrue(nspawn_version().major > 239)
        self.assertTrue(nspawn_version().major < 1000)

    def test_arch_version(self):
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
                NSpawnVersion(major=246, full="246.4-1-arch"),
                nspawn_version(),
            )
