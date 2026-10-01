#!/usr/bin/env python3
# Copyright (c) Meta Platforms, Inc. and affiliates.
#
# This source code is licensed under the MIT license found in the
# LICENSE file in the root directory of this source tree.


import json
import os
from pathlib import Path
from unittest import TestCase


def diff_ids(oci: Path) -> list[str]:
    """sha256 of every uncompressed layer tar, from the image config"""

    def blob(digest: str) -> dict[str, object]:
        algorithm, hex_digest = digest.split(":", 1)
        return json.loads((oci / "blobs" / algorithm / hex_digest).read_text())

    index = json.loads((oci / "index.json").read_text())
    manifest = blob(index["manifests"][0]["digest"])
    config = blob(manifest["config"]["digest"])
    return config["rootfs"]["diff_ids"]


class TestFastSnapshotDiff(TestCase):
    def test_layers_are_identical(self) -> None:
        full = diff_ids(Path(os.environ["OCI"]))
        fast = diff_ids(Path(os.environ["OCI_FAST_SNAPSHOT_DIFF"]))
        self.assertGreater(
            len(full),
            1,
            "at least one layer must be diffed against a parent phase",
        )
        self.assertEqual(full, fast)
