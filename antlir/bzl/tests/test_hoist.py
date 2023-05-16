#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import os
import unittest


def fs_tree(path):
    root = {}
    for (dirpath, _, filenames) in os.walk(path):
        parts = os.path.relpath(dirpath, path).split(os.sep)

        node = root
        for p in parts:
            node = node.setdefault(p, {})

        for fn in filenames:
            node[fn] = None
    return root


class HoistTest(unittest.TestCase):
    def test_simple_file(self):
        with importlib.resources.path(__package__, "test_simple_file") as path:
            self.assertTrue(os.path.isfile(path), "hoist file missing")

    def test_out_file(self):
        with importlib.resources.path(__package__, "test_out_file") as rpath:
            self.assertTrue(os.path.isdir(rpath), "hoist root is not a folder")

            ref = {
                ".": {
                    "file1": None,
                },
            }
            self.assertEqual(fs_tree(rpath), ref, "wrong hoist output")

    def test_simple_folder(self):
        with importlib.resources.path(__package__, "test_simple_folder") as rpath:
            ref = {
                ".": {
                    "file1.rpm": None,
                    "file2": None,
                },
            }
            self.assertEqual(fs_tree(rpath), ref, "wrong hoist output")

    def test_simple_selector(self):
        with importlib.resources.path(__package__, "test_simple_selector") as rpath:
            ref = {
                ".": {
                    "file1": None,
                    "file2.rpm": None,
                },
                "folder1": {
                    "file1.rpm": None,
                    "file2": None,
                },
            }
            self.assertEqual(fs_tree(rpath), ref, "wrong hoist output")

    def test_selector_flat(self):
        with importlib.resources.path(__package__, "test_selector_flat") as rpath:
            ref = {
                ".": {
                    "file1.rpm": None,
                    "file2.rpm": None,
                },
            }
            self.assertEqual(fs_tree(rpath), ref, "wrong hoist output")
