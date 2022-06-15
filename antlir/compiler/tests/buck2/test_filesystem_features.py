#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import json
import os
import unittest
from contextlib import contextmanager

from antlir.config import antlir_dep
from antlir.find_built_subvol import find_built_subvol
from antlir.tests.layer_resource import layer_resource, LAYER_SLASH_ENCODE
from antlir.tests.subvol_helpers import pop_path, render_subvol

TARGET_RESOURCE_PREFIX = "test_feature_"
TARGET_TO_PATH = {
    target[len(TARGET_RESOURCE_PREFIX) :]: path
    for target, path in [
        (
            target.replace(LAYER_SLASH_ENCODE, "/"),
            str(layer_resource(__package__, target)),
        )
        for target in importlib.resources.contents(__package__)
        if target.startswith(TARGET_RESOURCE_PREFIX)
    ]
}


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
                        "compiler/test_images/buck2/filesystem:" + target
                    ),
                },
            }
            if mount_config:
                expected_config.update(mount_config)
            with open(TARGET_TO_PATH[target] + "/mountconfig.json") as infile:
                self.assertEqual(expected_config, json.load(infile))
            yield find_built_subvol(TARGET_TO_PATH[target])

    def test_ensure_dirs_exist(self):
        with self.target_subvol("ensure-dirs-exist-layer") as subvol:
            rendered_subvol = render_subvol(subvol)

        ensure_dirs_exist_dir = pop_path(rendered_subvol, "ensure_dirs_exist")
        test_ensure_dirs_exist_dir = [
            "(Dir)",
            {
                "a": ["(Dir)", {}],
                "b": ["(Dir)", {"c": ["(Dir)", {}]}],
            },
        ]
        self.assertEquals(ensure_dirs_exist_dir, test_ensure_dirs_exist_dir)

    def test_ensure_subdirs_exist(self):
        with self.target_subvol("ensure-subdirs-exist-layer") as subvol:
            rendered_subvol = render_subvol(subvol)

        remove_dir = pop_path(rendered_subvol, "remove")
        test_remove_dir = [
            "(Dir)",
            {
                "a": ["(Dir)", {"b": ["(Dir)", {"c": ["(Dir)", {}]}]}],
                "b": ["(Dir)", {"c": ["(Dir)", {"d": ["(Dir)", {}]}]}],
                "c": ["(Dir)", {}],
            },
        ]
        self.assertEquals(remove_dir, test_remove_dir)

        ensure_subdirs_exist_dir = pop_path(
            rendered_subvol, "ensure_subdirs_exist"
        )
        test_ensure_subdirs_exist_dir = [
            "(Dir)",
            {
                "foo": [
                    "(Dir m555 o1111:1234)",
                    {
                        "bar": [
                            "(Dir m555 o1111:1234)",
                            {"baz": ["(Dir m555 o1111:1234)", {}]},
                        ]
                    },
                ]
            },
        ]
        self.assertEquals(
            ensure_subdirs_exist_dir, test_ensure_subdirs_exist_dir
        )

    def test_remove(self):
        with self.target_subvol("remove-layer") as subvol:
            rendered_subvol = render_subvol(subvol)

        remove_dir = pop_path(rendered_subvol, "remove")
        test_remove_dir = [
            "(Dir)",
            {
                "a": ["(Dir)", {"b": ["(Dir)", {}]}],
                "b": ["(Dir)", {}],
                "c": ["(Dir)", {}],
            },
        ]
        self.assertEquals(remove_dir, test_remove_dir)

    def test_symlink(self):
        with self.target_subvol("symlink-layer") as subvol:
            subvol_path = subvol.path()
            rendered_subvol = render_subvol(subvol)

        symlink_dir = pop_path(rendered_subvol, "symlink")
        test_symlink_dir = [
            "(Dir)",
            {
                "test_symlink_dir": ["(Symlink ../foo/bar)"],
                "test_symlink_file": ["(Symlink ../foo/bar/test_symlink.txt)"],
            },
        ]
        self.assertEquals(symlink_dir, test_symlink_dir)

        for path in [
            b"symlink/test_symlink_dir/test_symlink.txt",
            b"symlink/test_symlink_file",
        ]:
            self.assertEqual("symlink\n", (subvol_path / path).read_text())
