#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from .. import apply_plugins_to_popen, NspawnPlugin


class PluginsTestCase(unittest.TestCase):

    def test_apply_plugins_to_popen(self):
        self.assertEqual(
            'outer-inner-unwrapped',
            apply_plugins_to_popen(
                [
                    NspawnPlugin(popen=lambda x: 'inner-' + x),
                    NspawnPlugin(popen=lambda x: 'outer-' + x),
                ],
                'unwrapped',
            ),
        )
