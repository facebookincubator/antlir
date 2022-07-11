#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import json
import subprocess
import unittest
from contextlib import contextmanager

from antlir.config import antlir_dep
from antlir.find_built_subvol import find_built_subvol
from antlir.tests.subvol_helpers import pop_path, render_subvol

from .helpers import get_layer_target_to_path_by_prefix

TARGET_TO_PATH = get_layer_target_to_path_by_prefix(
    importlib.resources.contents(__package__), __package__, "test_layer_"
)


class ImageFeatureTestCase(unittest.TestCase):
    def setUp(self):
        # More output for easier debugging
        unittest.util._MAX_LENGTH = 12345
        self.maxDiff = 12345

    @contextmanager
    def target_subvol(self, target, mount_config=None):
        with self.subTest(target):
            # The mount configuration is very uniform, so we can check it here.
            expected_config = {
                "is_directory": True,
                "build_source": {
                    "type": "layer",
                    "source": antlir_dep(
                        "compiler/test_images/buck2/image_layer:" + target
                    ),
                },
            }
            if mount_config:
                expected_config.update(mount_config)
            with open(TARGET_TO_PATH[target] + "/mountconfig.json") as infile:
                self.assertEqual(expected_config, json.load(infile))
            yield find_built_subvol(TARGET_TO_PATH[target])

    def test_genrule_layer(self):
        with self.target_subvol("genrule-layer") as subvol:
            subvol_path = subvol.path()
            rendered_subvol = render_subvol(subvol)

        genrule_dir = pop_path(rendered_subvol, "genrule_output")
        self.assertIn("test_genrule_layer.txt", genrule_dir[1])

        self.assertEquals(
            subprocess.check_output(
                [
                    "cat",
                    str(subvol_path) + "/genrule_output/test_genrule_layer.txt",
                ]
            ),
            b"test_genrule_layer\n",
        )
