#!/usr/bin/env python3
# Copyright (c) Facebook, Inc. and its affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import tempfile
import unittest

from antlir.config import repo_config
from antlir.fs_utils import Path
from antlir.subvol_utils import TempSubvolumes
from antlir.tests.flavor_helpers import render_flavor
from antlir.tests.layer_resource import layer_resource, layer_resource_subvol
from antlir.tests.subvol_helpers import render_subvol

from ..compiler import build_image, parse_args


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
        meta = {
            ".meta": [
                "(Dir)",
                {
                    "flavor": [render_flavor(flavor="antlir_test")],
                    "private": [
                        "(Dir)",
                        {
                            "opts": [
                                "(Dir)",
                                {"artifacts_may_require_repo": ["(File d2)"]},
                            ]
                        },
                    ],
                },
            ]
        }
        # The parent has a couple of directories.
        self.assertEqual(
            ["(Dir)", {"a": ["(Dir)", {"b": ["(Dir)", {}]}], **meta}],
            render_subvol(layer_resource_subvol(__package__, "parent")),
        )
        # The child is near-empty because the `remove_paths` cleaned it up.
        self.assertEqual(
            ["(Dir)", {**meta}],
            render_subvol(layer_resource_subvol(__package__, "child")),
        )

    def test_conflict(self):
        build_appliance_layer_path = layer_resource(
            __package__, "test-build-appliance"
        )
        with TempSubvolumes() as tmp_subvols, (
            tempfile.NamedTemporaryFile()
        ) as tf, Path.resource(
            __package__, "feature_both", exe=False
        ) as feature_both, Path.resource(
            __package__, "feature_add", exe=False
        ) as feature_add, Path.resource(
            __package__, "feature_remove", exe=False
        ) as feature_remove, self.assertRaisesRegex(
            # Removes get built before adds; a conflict means nothing to remove
            AssertionError,
            "Path does not exist",
        ):
            flavor_config = repo_config().flavor_to_config["antlir_test"]

            # Write the targets_and_outputs file
            tf.write(
                Path.json_dumps(
                    {
                        _test_feature_target(
                            "feature_addremove_conflict_add"
                        ): feature_add,
                        _test_feature_target(
                            "feature_addremove_conflict_remove"
                        ): feature_remove,
                        flavor_config.build_appliance: (
                            build_appliance_layer_path
                        ),
                    }
                ).encode()
            )
            tf.seek(0)

            subvol = tmp_subvols.external_command_will_create("test_conflict")
            # We cannot make this an `image.layer` target, since Buck
            # doesn't (yet) have a nice story for testing targets whose
            # builds are SUPPOSED to fail.
            build_image(
                parse_args(
                    [
                        "--subvolumes-dir",
                        subvol.path().dirname(),
                        "--subvolume-rel-path",
                        subvol.path().basename(),
                        "--child-layer-target",
                        "unused",
                        f"--child-feature-json={feature_both}",
                        "--targets-and-outputs",
                        tf.name,
                        "--flavor-config",
                        flavor_config.json(),
                    ]
                )
            )
