#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from fs_image.btrfs_diff.tests.render_subvols import (
    check_common_rpm_render,
    pop_path,
    render_sendstream,
)
from fs_image.tests.layer_resource import layer_resource_subvol


class InstallToyRpmTestCase(unittest.TestCase):
    def test_contents(self):
        self.maxDiff = None
        sv = layer_resource_subvol(__package__, "install-toy-rpm")
        r = render_sendstream(sv.mark_readonly_and_get_sendstream())

        self.assertEqual(
            [
                "(Dir)",
                {"bin": ["(Dir)", {"toy_src_file": ["(File m755 d40)"]}]},
            ],
            pop_path(r, "usr"),
        )

        check_common_rpm_render(self, r, "yum")
