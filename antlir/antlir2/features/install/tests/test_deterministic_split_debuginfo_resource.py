#!/usr/bin/env fbpython
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.

import hashlib
import json
import platform
import unittest
from pathlib import Path

EXPECTED_HASHES = {
    "x86_64": {
        "build_id": "cdede5c8d904566a5cd33f0fdaff40eab44cdcd3",
        "stripped_hash": "fd17e87da8106206c67f040707170426",
        "debug_hash": "d299324457ed9ce850b4d62546ae3487",
    },
    "aarch64": {
        "build_id": "b10f090fd2b667eb80e46d68a3233976f0013258",
        "stripped_hash": "259c762a7e91223aaab3119dabc5d00c",
        "debug_hash": "61b7b37ddb917095d853f48ee1572966",
    },
}

RESOURCES_JSON = Path("/binary-with-deterministic-resource.resources.json")


class TestDeterministicSplitDebuginfoResource(unittest.TestCase):
    """Test that resources which are ELF binaries get their debuginfo split."""

    def _get_expected(self):
        machine = platform.machine()
        if machine not in EXPECTED_HASHES:
            self.skipTest(f"Unsupported architecture: {machine}")
        return EXPECTED_HASHES[machine]

    def test_resource_debuginfo_exists(self):
        expected = self._get_expected()
        build_id = expected["build_id"]
        debug_path = Path(
            f"/usr/lib/debug/.build-id/{build_id[:2]}/{build_id[2:]}.debug"
        )

        self.assertTrue(
            debug_path.exists(),
            f"Debuginfo for resource binary should exist at {debug_path}",
        )
        self.assertTrue(
            debug_path.is_file(),
            f"{debug_path} should be a regular file",
        )

        with open(debug_path, "rb") as f:
            debug_hash = hashlib.md5(f.read()).hexdigest()
        self.assertEqual(
            debug_hash,
            expected["debug_hash"],
            "Debuginfo hash should match expected value",
        )

    def test_resource_binary_is_stripped(self):
        expected = self._get_expected()

        self.assertTrue(
            RESOURCES_JSON.exists(),
            f"Resources manifest should exist at {RESOURCES_JSON}",
        )
        with open(RESOURCES_JSON) as f:
            resources = json.load(f)

        resource_values = list(resources.values())
        self.assertEqual(len(resource_values), 1, "Expected exactly one resource")
        resource_path = Path("/") / resource_values[0]
        self.assertTrue(
            resource_path.exists(),
            f"Resource binary should exist at {resource_path}",
        )

        with open(resource_path, "rb") as f:
            actual_hash = hashlib.md5(f.read()).hexdigest()
        self.assertEqual(
            actual_hash,
            expected["stripped_hash"],
            f"Resource binary should be stripped (hash {actual_hash} != expected {expected['stripped_hash']})",
        )
