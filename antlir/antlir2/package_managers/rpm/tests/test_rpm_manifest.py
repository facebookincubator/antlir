# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import importlib.resources
import json
import unittest


class TestRpmManifest(unittest.TestCase):
    def setUp(self) -> None:
        super().setUp()
        self.maxDiff = None

    def test_manifest(self) -> None:
        with importlib.resources.open_text(__package__, "manifest.json") as f:
            manifest = json.load(f)
        self.assertEqual(
            manifest,
            {
                "rpms": [
                    {
                        "name": "foo-empty",
                        "nevra": {
                            "arch": "noarch",
                            "epochnum": 0,
                            "name": "foo-empty",
                            "release": "1",
                            "version": "3",
                        },
                        "patched_cves": [],
                        "os": "linux",
                        "size": 0,
                        "source_rpm": "foo-empty-3-1.src.rpm",
                    },
                    {
                        "name": "foo",
                        "nevra": {
                            "arch": "noarch",
                            "epochnum": 0,
                            "name": "foo",
                            "release": "1",
                            "version": "3",
                        },
                        "patched_cves": ["CVE-2024-0101"],
                        "os": "linux",
                        "size": 0,
                        "source_rpm": "foo-3-1.src.rpm",
                    },
                    {
                        "name": "foobar",
                        "nevra": {
                            "arch": "noarch",
                            "epochnum": 0,
                            "name": "foobar",
                            "release": "1",
                            "version": "3",
                        },
                        "patched_cves": [],
                        "os": "linux",
                        "size": 0,
                        "source_rpm": "foobar-3-1.src.rpm",
                    },
                    {
                        "name": "foobarbaz",
                        "nevra": {
                            "arch": "noarch",
                            "epochnum": 0,
                            "name": "foobarbaz",
                            "release": "1",
                            "version": "3",
                        },
                        "patched_cves": [],
                        "os": "linux",
                        "size": 0,
                        "source_rpm": "foobarbaz-3-1.src.rpm",
                    },
                ]
            },
        )
