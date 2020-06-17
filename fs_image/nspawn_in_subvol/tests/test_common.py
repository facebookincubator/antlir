#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from unittest import mock

from ..common import apply_wrappers_to_popen, nspawn_version, NspawnWrapper


class CommonTestCase(unittest.TestCase):

    def test_nspawn_version(self):
        with mock.patch('subprocess.check_output') as version:
            version.return_value = (
                'systemd 602214076 (v602214076-2.fb1)\n+AVOGADROS SYSTEMD\n')
            self.assertEqual(602214076, nspawn_version())

        # Check that the real nspawn on the machine running this test is
        # actually a sane version.  We need at least 239 to do anything useful
        # and 1000 seems like a reasonable upper bound, but mostly I'm just
        # guessing here.
        self.assertTrue(nspawn_version() > 239)
        self.assertTrue(nspawn_version() < 1000)

    def test_apply_wrappers_to_popen(self):
        self.assertEqual(
            'outer-inner-unwrapped',
            apply_wrappers_to_popen(
                [
                    NspawnWrapper(popen=lambda x: 'inner-' + x),
                    NspawnWrapper(popen=lambda x: 'outer-' + x),
                ],
                'unwrapped',
            ),
        )
