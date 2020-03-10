#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import json
import unittest

from io import StringIO

from fs_image.layer_mount_config import main


class TestLayerMountConfig(unittest.TestCase):

    def test_error(self):
        out = StringIO()
        with self.assertRaisesRegex(RuntimeError, '`build_source` must not '):
            main(StringIO('{"build_source": "bad"}'), out, '//layer:path')
        self.assertEqual('', out.getvalue())

    def test_config_merging(self):
        out = StringIO()
        main(StringIO('{"runtime_source": "meow"}'), out, '//layer:path')
        self.assertEqual({
            'runtime_source': 'meow',
            'is_directory': True,
            'build_source': {'source': '//layer:path', 'type': 'layer'},
        }, json.loads(out.getvalue()))
