#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import unittest

from antlir.tests.layer_resource import layer_resource_subvol
from antlir.tests.subvol_helpers import get_meta_dir_contents, render_subvol


def _test_feature_target(feature_target):
    return (
        "//antlir/compiler/test_images:"
        + feature_target
        + "_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN"
    )


class AddRemoveConflictTestCase(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    def test_check_layers(self):
        meta_parent = get_meta_dir_contents(
            subvol=layer_resource_subvol(__package__, "parent")
        )
        meta_child = get_meta_dir_contents(
            subvol=layer_resource_subvol(__package__, "child")
        )

        # The parent has a couple of directories.
        self.assertEqual(
            [
                "(Dir)",
                {"a": ["(Dir)", {"b": ["(Dir)", {}]}], ".meta": meta_parent},
            ],
            render_subvol(layer_resource_subvol(__package__, "parent")),
        )
        # The child is near-empty because the `remove_paths` cleaned it up.
        self.assertEqual(
            ["(Dir)", {".meta": meta_child}],
            render_subvol(layer_resource_subvol(__package__, "child")),
        )
