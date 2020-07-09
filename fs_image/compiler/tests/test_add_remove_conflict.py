#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import subprocess
import tempfile
import unittest

from fs_image.btrfs_diff.tests.render_subvols import render_sendstream
from fs_image.find_built_subvol import find_built_subvol, volume_dir

from ..compiler import build_image, parse_args


def _test_feature_target(feature_target):
    return (
        "//fs_image/compiler/test_images:"
        + feature_target
        + (
            "_IF_YOU_REFER_TO_THIS_RULE_YOUR_DEPENDENCIES_WILL_BE_BROKEN_"
            "SO_DO_NOT_DO_THIS_EVER_PLEASE_KTHXBAI"
        )
    )


class AddRemoveConflictTestCase(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    def _resource_path(self, name: str):
        return os.path.join(
            # This works even in @mode/opt because the test is a XAR
            os.path.dirname(__file__),
            "data/" + name,
        )

    def test_check_layers(self):
        meta = {
            "meta": [
                "(Dir)",
                {
                    "private": [
                        "(Dir)",
                        {
                            "opts": [
                                "(Dir)",
                                {"artifacts_may_require_repo": ["(File d2)"]},
                            ]
                        },
                    ]
                },
            ]
        }
        # The parent has a couple of directories.
        self.assertEqual(
            ["(Dir)", {"a": ["(Dir)", {"b": ["(Dir)", {}]}], **meta}],
            render_sendstream(
                find_built_subvol(
                    self._resource_path("parent")
                ).mark_readonly_and_get_sendstream()
            ),
        )
        # The child is near-empty because the `remove_paths` cleaned it up.
        self.assertEqual(
            ["(Dir)", {**meta}],
            render_sendstream(
                find_built_subvol(
                    self._resource_path("child")
                ).mark_readonly_and_get_sendstream()
            ),
        )

    def test_conflict(self):
        # Future: de-duplicate this with TempSubvolumes, perhaps?
        tmp_parent = os.path.join(volume_dir(), "tmp")
        try:
            os.mkdir(tmp_parent)
        except FileExistsError:
            pass
        # Removes get built before adds, so a conflict means nothing to remove
        with tempfile.TemporaryDirectory(
            dir=tmp_parent
        ) as temp_subvol_dir, self.assertRaisesRegex(
            AssertionError, "Path does not exist"
        ):
            try:
                # We cannot make this an `image.layer` target, since Buck
                # doesn't (yet) have a nice story for testing targets whose
                # builds are SUPPOSED to fail.
                build_image(
                    parse_args(
                        [
                            "--subvolumes-dir",
                            temp_subvol_dir,
                            "--subvolume-rel-path",
                            "SUBVOL",
                            "--child-layer-target",
                            "unused",
                            "--child-feature-json",
                            self._resource_path("feature_both"),
                            "--child-dependencies",
                            _test_feature_target(
                                "feature_addremove_conflict_add"
                            ),
                            self._resource_path("feature_add"),
                            _test_feature_target(
                                "feature_addremove_conflict_remove"
                            ),
                            self._resource_path("feature_remove"),
                        ]
                    )
                )
            finally:
                # Ignore error code in case something broke early in the test
                subprocess.run(
                    [
                        "sudo",
                        "btrfs",
                        "subvolume",
                        "delete",
                        os.path.join(temp_subvol_dir, "SUBVOL"),
                    ]
                )
