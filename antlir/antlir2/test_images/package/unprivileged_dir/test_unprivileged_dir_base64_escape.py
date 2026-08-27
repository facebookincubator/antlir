# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import importlib.resources
import json
import os
from pathlib import Path

from later.unittest import TestCase


class TestUnprivilegedDir(TestCase):
    def setUp(self) -> None:
        self.maxDiff = None

    def test_base64(self) -> None:
        """Test that only filenames with invalid characters are base64 encoded."""
        root = Path("/unprivileged_dir").resolve()
        dirs = set()
        files = set()
        raw_paths = set()
        for dirpath, dirnames, filenames in root.walk():
            for dirname in dirnames:
                item = dirpath / dirname
                rel = item.relative_to(root)
                raw_paths.add(str(rel))
                dirs.add(str(rel))
            for filename in filenames:
                item = dirpath / filename
                rel = item.relative_to(root)
                raw_paths.add(str(rel))
                files.add(str(rel))

        self.assertEqual({".meta", "baz", "ZXNjYXBlXHgyZGRpcg=="}, dirs)
        self.assertEqual(
            {
                "Zm9vXHgyZGJhcg==",
                "ZXNjYXBlXHgyZGRpcg==/component",
                "baz/doesnt_need_escaping",
                "link_to_escaped",
                ".meta/target",
            },
            files,
        )
        self.assertEqual(
            {
                "Zm9vXHgyZGJhcg==",  # foo\x2dbar (encoded)
                "ZXNjYXBlXHgyZGRpcg==",  # /escape\\x2ddir (encoded)
                "ZXNjYXBlXHgyZGRpcg==/component",
                # all not encoded
                "baz",
                "baz/doesnt_need_escaping",
                "link_to_escaped",
                ".meta",
                ".meta/target",
            },
            raw_paths,
        )
        # We don't currently do any special handling for symlinks so we expect them to
        # still point to the unescaped path
        self.assertEqual(
            os.readlink(root / "link_to_escaped"),
            r"/escape\x2ddir",
        )

    def test_escaped_paths_mapping(self) -> None:
        with importlib.resources.path(__package__, "escaped_paths_mapping") as path:
            with open(path) as f:
                self.assertEqual(
                    {
                        "/Zm9vXHgyZGJhcg==": "/foo\\x2dbar",
                        "/ZXNjYXBlXHgyZGRpcg==": "/escape\\x2ddir",
                        "/ZXNjYXBlXHgyZGRpcg==/component": "/escape\\x2ddir/component",
                    },
                    json.load(f),
                )
