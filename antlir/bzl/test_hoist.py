#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import os
import unittest

from pkg_resources import resource_filename


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
        path = resource_filename(__name__, "test_simple_file")
        self.assertTrue(os.path.isfile(path), "hoist file missing")

    def test_out_file(self):
        rpath = resource_filename(__name__, "test_out_file")
        self.assertTrue(os.path.isdir(rpath), "hoist root is not a folder")

        ref = {
            ".": {
                "file1": None,
            },
        }
        self.assertEqual(fs_tree(rpath), ref, "wrong hoist output")

    def test_simple_folder(self):
        rpath = resource_filename(__name__, "test_simple_folder")

        ref = {
            ".": {
                "file1.rpm": None,
                "file2": None,
            },
        }
        self.assertEqual(fs_tree(rpath), ref, "wrong hoist output")

    def test_simple_selector(self):
        rpath = resource_filename(__name__, "test_simple_selector")

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
        rpath = resource_filename(__name__, "test_selector_flat")

        ref = {
            ".": {
                "file1.rpm": None,
                "file2.rpm": None,
            },
        }
        self.assertEqual(fs_tree(rpath), ref, "wrong hoist output")
