#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import unittest


class InstallSignedToyRpmTestCase(unittest.TestCase):
    def test_contents(self):
        self.assertTrue(os.path.exists("/usr/bin/toy_src_file"))
        self.assertFalse(os.path.exists("/antlir-rpm-gpg-keys"))

    def test_rpm_signature(self):
        info = subprocess.check_output(
            ["rpm", "-q", "toy", "--queryformat", "%{SIGPGP:pgpsig}"], text=True
        )
        self.assertRegex(info, "RSA/SHA256, .*, Key ID 4785998712764132")

    def test_key_import(self):
        keys = subprocess.check_output(
            ["rpm", "-q", "gpg-pubkey", "--queryformat", "%{SUMMARY}"],
            text=True,
        )
        self.assertIn("Test Key <key@example.com>", keys)
